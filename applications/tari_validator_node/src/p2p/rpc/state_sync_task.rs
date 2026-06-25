//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::num::NonZeroUsize;

use log::*;
use tari_ootle_common_types::{Epoch, optional::Optional, shard::Shard};
use tari_ootle_p2p::proto::rpc;
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{StateTransition, StateVersionTransitions, SubstateValueFilterFlags},
};
use tari_rpc_framework::RpcStatus;
use tari_state_tree::Version;
use tokio::sync::mpsc;

const LOG_TARGET: &str = "tari::ootle::rpc::sync_task";

pub struct StateSyncTask<TStateStore: StateStore> {
    store: TStateStore,
    sender: mpsc::Sender<Result<rpc::SyncStateResponse, RpcStatus>>,
    shard: Shard,
    start_state_version: Version,
    end_epoch: Option<Epoch>,
    current_epoch: Epoch,
    batch_size: NonZeroUsize,
    value_filters: SubstateValueFilterFlags,
}

impl<TStateStore: StateStore> StateSyncTask<TStateStore> {
    pub fn new(
        store: TStateStore,
        sender: mpsc::Sender<Result<rpc::SyncStateResponse, RpcStatus>>,
        shard: Shard,
        start_state_version: Version,
        end_epoch: Option<Epoch>,
        current_epoch: Epoch,
        batch_size: NonZeroUsize,
        value_filters: SubstateValueFilterFlags,
    ) -> Self {
        Self {
            store,
            sender,
            shard,
            start_state_version,
            end_epoch,
            current_epoch,
            batch_size,
            value_filters,
        }
    }

    pub async fn run(mut self) -> Result<(), ()> {
        // For an unbounded (sync-to-tip) request, snapshot the committed tree tip before scanning. The
        // completion marker advances the client over trailing versions that stream no updates (all
        // filtered out for its subscription). Snapshotting first ensures the marker never reports
        // beyond what we streamed: anything committed after this point is left for the next round.
        let tip_at_start = if self.end_epoch.is_none() {
            self.read_latest_tree_version()
        } else {
            None
        };

        let mut current_state_version = self.start_state_version;
        let mut counter = 0usize;
        let mut last_sent_version: Option<Version> = None;
        loop {
            match self.fetch_next_batch(current_state_version) {
                Ok(Some(transitions)) => {
                    if let Some(end_epoch) = self.end_epoch {
                        // TODO(perf): might be better to not load in the first place, however also might incur the cost
                        // of a db index, more complex keys or loading from db anyway
                        if transitions.epoch > end_epoch {
                            info!(target: LOG_TARGET, "🌍 Reached end of requested epoch: {}", end_epoch);
                            break;
                        }
                    }
                    if !transitions.updates.is_empty() {
                        debug!(target: LOG_TARGET, "🌍 Fetched {} state transition(s) up to v{}", transitions.updates.len(), transitions.state_version);
                    }

                    current_state_version = transitions.state_version + 1;
                    counter += transitions.updates.len();

                    let state_version = transitions.state_version;
                    let has_updates = !transitions.updates.is_empty();
                    self.send_batches(transitions).await?;
                    // A version whose updates are all filtered out streams no batch, so only versions we
                    // actually sent count towards the client's recorded progress.
                    if has_updates {
                        last_sent_version = Some(state_version);
                    }
                },
                Ok(None) => {
                    // TODO: differentiate between not found and end of stream
                    debug!(target: LOG_TARGET, "🌍sync complete ({}). {} update(s) sent.", current_state_version, counter);
                    break;
                },
                Err(err) => {
                    error!(target: LOG_TARGET, "🌍 Error fetching state transitions: {}", err);
                    self.send(Err(RpcStatus::log_internal_error(LOG_TARGET)(err))).await?;
                    return Err(());
                },
            }
        }

        self.send_complete(tip_at_start, last_sent_version).await
    }

    fn read_latest_tree_version(&self) -> Option<Version> {
        match self
            .store
            .with_read_tx(|tx| tx.state_tree_versions_get_latest(self.shard))
        {
            Ok(version) => version,
            Err(err) => {
                // Non-fatal: the completion marker is an optimisation; streamed batches are unaffected.
                warn!(target: LOG_TARGET, "🌍 Failed to read latest tree version for {}: {}", self.shard, err);
                None
            },
        }
    }

    /// Terminates every stream with a `SyncComplete` stating the version the client is now synced to.
    ///
    /// For an unbounded request this is the committed tree tip (capped to what we streamed), letting the
    /// client advance over trailing versions that streamed no updates - e.g. a shard whose latest
    /// transitions are all substate types the client filtered out. Such a shard otherwise streams no
    /// message at all, so the client could never observe that it has caught up and would re-scan it from
    /// scratch every round, leaving any version comparison against the committed version unsatisfiable.
    ///
    /// For a bounded request the consumer verifies against its own checkpoint, so the reported version is
    /// just our last streamed version - the consumer does not trust it as the sync target.
    async fn send_complete(
        &mut self,
        tip_at_start: Option<Version>,
        last_sent_version: Option<Version>,
    ) -> Result<(), ()> {
        let synced_to_version = match tip_at_start {
            // Unbounded: advance to the committed tip, but never past a version we actually streamed.
            Some(tip) => tip.max(last_sent_version.unwrap_or(0)),
            // Bounded, or an unbounded shard with no committed state: report the last streamed version.
            None => last_sent_version.unwrap_or_else(|| self.start_state_version.saturating_sub(1)),
        };

        self.send(Ok(rpc::SyncStateResponse {
            response: Some(rpc::sync_state_response::Response::Complete(rpc::SyncComplete {
                synced_to_version,
                epoch: Some(self.current_epoch.into()),
            })),
        }))
        .await
    }

    fn fetch_next_batch(
        &self,
        current_state_version: Version,
    ) -> Result<Option<StateVersionTransitions>, StorageError> {
        let transitions = self.store.with_read_tx(|tx| {
            StateTransition::get_for_shard(tx, self.shard, current_state_version, self.value_filters).optional()
        })?;
        Ok(transitions)
    }

    async fn send(&mut self, result: Result<rpc::SyncStateResponse, RpcStatus>) -> Result<(), ()> {
        if self.sender.send(result).await.is_err() {
            debug!(
                target: LOG_TARGET,
                "Peer stream closed by client before completing. Aborting"
            );
            return Err(());
        }
        Ok(())
    }

    async fn send_batches(&mut self, transitions: StateVersionTransitions) -> Result<(), ()> {
        let chunks = transitions.into_chunks(self.batch_size);
        let num_chunks = chunks.len();

        for (i, chunk) in chunks.into_iter().enumerate() {
            let updates = chunk.updates.into_iter().map(Into::into).collect();

            self.send(Ok(rpc::SyncStateResponse {
                response: Some(rpc::sync_state_response::Response::Batch(rpc::SubstateBatch {
                    state_version: chunk.state_version,
                    updates,
                    has_more: i < num_chunks - 1,
                    epoch: Some(chunk.epoch.into()),
                })),
            }))
            .await?;
        }

        Ok(())
    }
}
