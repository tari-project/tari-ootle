//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::num::NonZeroUsize;

use log::*;
use tari_ootle_common_types::{optional::Optional, shard::Shard, Epoch};
use tari_ootle_p2p::proto::rpc;
use tari_ootle_storage::{
    consensus_models::{StateTransition, StateVersionTransitions, SubstateValueFilterFlags},
    StateStore,
    StorageError,
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
        batch_size: NonZeroUsize,
        value_filters: SubstateValueFilterFlags,
    ) -> Self {
        Self {
            store,
            sender,
            shard,
            start_state_version,
            end_epoch,
            batch_size,
            value_filters,
        }
    }

    pub async fn run(mut self) -> Result<(), ()> {
        let mut current_state_version = self.start_state_version;
        let mut counter = 0usize;
        loop {
            match self.fetch_next_batch(current_state_version) {
                Ok(Some(transitions)) => {
                    info!(target: LOG_TARGET, "🌍 Fetched {} state transition(s) up to v{}", transitions.updates.len(), transitions.state_version);
                    if let Some(end_epoch) = self.end_epoch {
                        // TODO(perf): might be better to not load in the first place, however also might incur the cost
                        // of a db index, more complex keys or loading from db anyway
                        if transitions.epoch > end_epoch {
                            info!(target: LOG_TARGET, "🌍 Reached end of requested epoch: {}", end_epoch);
                            return Ok(());
                        }
                    }

                    current_state_version = transitions.state_version + 1;
                    counter += transitions.updates.len();

                    self.send_responses(transitions).await?;
                },
                Ok(None) => {
                    // TODO: differentiate between not found and end of stream
                    // self.send(Err(RpcStatus::not_found(format!(
                    //     "State transition not found with id={current_state_version}"
                    // ))))
                    // .await?;

                    debug!(target: LOG_TARGET, "🌍sync complete ({}). {} update(s) sent.", current_state_version, counter);
                    // Finished
                    return Ok(());
                },
                Err(err) => {
                    self.send(Err(RpcStatus::log_internal_error(LOG_TARGET)(err))).await?;
                    return Err(());
                },
            }
        }
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

    async fn send_responses(&mut self, transitions: StateVersionTransitions) -> Result<(), ()> {
        let chunks = transitions.into_chunks(self.batch_size);
        let num_chunks = chunks.len();

        for (i, chunk) in chunks.into_iter().enumerate() {
            let updates = chunk.updates.into_iter().map(Into::into).collect();

            self.send(Ok(rpc::SyncStateResponse {
                state_version: chunk.state_version,
                updates,
                has_more: i < num_chunks - 1,
                epoch: Some(chunk.epoch.into()),
            }))
            .await?;
        }

        Ok(())
    }
}
