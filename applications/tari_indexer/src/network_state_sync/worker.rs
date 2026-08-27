//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    pin::pin,
    sync::Arc,
};

use futures::StreamExt;
use log::*;
use ootle_network::Network;
#[cfg(feature = "metrics")]
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_engine_types::{
    published_template::PublishedTemplateMetadata,
    substate::{SubstateId, SubstateValue},
    transaction_receipt::TransactionReceipt,
};
use tari_epoch_manager::{EpochManagerEvent, EpochManagerReader, service::EpochManagerHandle};
use tari_indexer_client::event::{IndexerEvent, NewEpochEvent, TransactionEvent, TransactionFinalizedEvent};
use tari_indexer_lib::substate_versions::SubstateVersionTracker;
use tari_networking::NetworkingHandle;
use tari_ootle_common_types::{Epoch, ShardGroup, StateVersion, VotePower, optional::Optional, shard::Shard};
use tari_ootle_p2p::{PeerAddress, TariMessagingSpec, proto::rpc};
use tari_ootle_storage::{
    StorageError,
    consensus_models::{
        EpochCheckpoint,
        SubstateData,
        SubstateUpdateProof,
        SubstateValueFilterFlags,
        VerifiedBlockTip,
    },
};
use tari_ootle_transaction::TransactionId;
use tari_rpc_framework::__macro_reexports::future::Either;
use tari_shutdown::ShutdownSignal;
use tari_template_lib_types::{Amount, TemplateAddress, TransactionReceiptAddress};
use tokio::{sync::broadcast, time};

#[cfg(feature = "metrics")]
use crate::{network_state_sync::NetworkStateMetrics, store::ReadOnlyStore};
use crate::{
    network_state_sync::{
        committee_client::{ValidatorCommitteeRpcPool, ValidatorRpcSession},
        config::NetworkWideStateSyncConfig,
        error::NetworkStateSyncError,
        stats::SyncStats,
        sync_plan::SyncPlan,
        sync_progress::SyncProgress,
        validator_status::ValidatorStatusMonitor,
    },
    notify::Notify,
    storage_sqlite::{
        SqliteIndexerStore,
        SqliteStoreWriteTransaction,
        models::{Key, UtxoSpent, UtxoUnspent, UtxoUpdateRecord, VerifiedStateRoot},
    },
    store::{
        IndexerStore,
        IndexerStoreReadTransaction,
        IndexerStoreReader,
        IndexerStoreWriteTransaction,
        InsertedEvent,
    },
};

const LOG_TARGET: &str = "tari::indexer::network_state_sync::worker";

#[derive(Clone)]
pub struct NetworkWideStateSync {
    network: Network,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    networking: NetworkingHandle<TariMessagingSpec>,
    store: SqliteIndexerStore,
    stats: SyncStats,
    config: NetworkWideStateSyncConfig,
    notify: Notify<IndexerEvent>,
    transaction_event_notify: Notify<TransactionEvent>,
    validator_status: ValidatorStatusMonitor,
    substate_versions: Arc<SubstateVersionTracker>,
    #[cfg(feature = "metrics")]
    metrics: NetworkStateMetrics,
    #[cfg(feature = "metrics")]
    consensus_constants: ConsensusConstants,
}

impl NetworkWideStateSync {
    pub fn new(
        network: Network,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        networking: NetworkingHandle<TariMessagingSpec>,
        storage: SqliteIndexerStore,
        config: NetworkWideStateSyncConfig,
        notify: Notify<IndexerEvent>,
        transaction_event_notify: Notify<TransactionEvent>,
        validator_status: ValidatorStatusMonitor,
        substate_versions: Arc<SubstateVersionTracker>,
        #[cfg(feature = "metrics")] metrics: NetworkStateMetrics,
        #[cfg(feature = "metrics")] consensus_constants: ConsensusConstants,
    ) -> Self {
        Self {
            network,
            epoch_manager,
            networking,
            store: storage,
            stats: SyncStats::new(),
            config,
            notify,
            transaction_event_notify,
            validator_status,
            substate_versions,
            #[cfg(feature = "metrics")]
            metrics,
            #[cfg(feature = "metrics")]
            consensus_constants,
        }
    }

    pub fn spawn(mut self, shutdown_signal: ShutdownSignal) -> tokio::task::JoinHandle<()> {
        let mut epoch_events = self.epoch_manager.subscribe();
        tokio::spawn(async move {
            loop {
                let config = self.config.clone();
                let task = self.start(&mut epoch_events);
                let task = pin!(task);
                match shutdown_signal.clone().select(task).await {
                    Either::Left(_) => {
                        info!(target: LOG_TARGET, "🌍️ Network-wide state sync was shutdown.");
                        break;
                    },
                    Either::Right((Ok(()), _)) => {
                        info!(target: LOG_TARGET, "🌍️ Network-wide state sync completed successfully.");
                    },
                    Either::Right((Err(e), _)) => {
                        error!(target: LOG_TARGET, "⚠️ Network-wide state sync failed: {}", e);
                        // Restart after cooldown
                        time::sleep(config.work_interval).await;
                    },
                }
            }
        })
    }

    async fn start(
        &mut self,
        epoch_events: &mut broadcast::Receiver<EpochManagerEvent>,
    ) -> Result<(), NetworkStateSyncError> {
        self.epoch_manager.wait_for_initial_scanning_to_complete().await?;

        // Publish last-known totals immediately so a freshly restarted indexer reports them before its
        // first sync round completes.
        #[cfg(feature = "metrics")]
        self.update_metrics().await;

        let mut interval = time::interval(self.config.work_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                Ok(event) = epoch_events.recv() => {
                    interval.reset();
                    self.handle_epoch_event(event).await?;
                },
                _ = interval.tick() => {
                    self.start_sync_round().await?;
                }
            }
        }
    }

    async fn start_sync_round(&mut self) -> Result<(), NetworkStateSyncError> {
        info!(target: LOG_TARGET, "🌍️ Starting network-wide state sync round...");
        let sync_plan = self.initialize_sync_plan().await?;
        if sync_plan.network_description().epoch.is_zero() {
            info!(target: LOG_TARGET, "🌍️ Current epoch is zero, nothing to sync.");
            return Ok(());
        }
        self.start_sync(sync_plan).await?;
        self.stats.log_stats();
        self.stats.reset();
        #[cfg(feature = "metrics")]
        self.update_metrics().await;
        Ok(())
    }

    /// Reads the persisted economic totals and publishes them to the Prometheus gauges. Metrics are
    /// observability only, so a read failure is logged rather than propagated into the sync loop.
    #[cfg(feature = "metrics")]
    async fn update_metrics(&self) {
        match ReadOnlyStore::new(self.store.clone()).get_tari_economics().await {
            Ok(economics) => {
                let current_epoch = self.epoch_manager.get_current_epoch();
                let target_burn_rate_bps = self.consensus_constants.exhaust_burn_rate(current_epoch).as_bps();
                self.metrics.update(&economics, target_burn_rate_bps);
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "⚠️ Failed to update network economics metrics: {err}");
            },
        }
    }

    async fn handle_epoch_event(&mut self, event: EpochManagerEvent) -> Result<(), NetworkStateSyncError> {
        match event {
            EpochManagerEvent::EpochChanged { epoch, .. } => {
                info!(target: LOG_TARGET, "🌍️ Epoch changed to {}.", epoch);
                self.notify.notify(NewEpochEvent { epoch });
                self.start_sync_round().await?;
            },
        }
        Ok(())
    }

    async fn initialize_sync_plan(&self) -> Result<SyncPlan, NetworkStateSyncError> {
        let network_desc = self.epoch_manager.get_network_description().await?;
        let sync_progress = self
            .store
            .with_read_tx(|tx| tx.key_value_get_value::<_, SyncProgress>(Key::SyncProgress))
            .await
            .optional()?
            .unwrap_or_default();

        let mut committee_pools = HashMap::with_capacity(network_desc.num_committees());
        for shard_group in network_desc.shard_groups_iter() {
            let pool = ValidatorCommitteeRpcPool::new(shard_group, self.networking.clone(), self.epoch_manager.clone());
            committee_pools.insert(shard_group, pool);
        }

        Ok(SyncPlan::new(network_desc, sync_progress, committee_pools))
    }

    async fn start_sync(&mut self, mut sync_plan: SyncPlan) -> Result<(), NetworkStateSyncError> {
        self.sync_checkpoints(&mut sync_plan).await?;
        self.sync_state(&mut sync_plan).await?;

        Ok(())
    }

    #[expect(clippy::too_many_lines)]
    async fn sync_checkpoints(&mut self, sync_plan_mut: &mut SyncPlan) -> Result<(), NetworkStateSyncError> {
        let prev_epoch = sync_plan_mut
            .network_description()
            .epoch()
            .checked_sub(Epoch(1))
            .ok_or_else(|| NetworkStateSyncError::InvariantError {
                details: "current epoch is zero, there are no checkpoints to sync".to_string(),
            })?;
        let committee_pools = sync_plan_mut.committee_pools().clone();

        for (shard_group, mut pool) in committee_pools {
            let from_epoch = sync_plan_mut
                .sync_progress()
                .checkpoint_progress
                .get(&shard_group)
                .copied()
                .unwrap_or_else(Epoch::zero);
            if from_epoch >= prev_epoch {
                info!(target: LOG_TARGET, "🌍️ No checkpoints to sync for shard group {shard_group} from epoch {from_epoch}");
                continue;
            }
            info!(target: LOG_TARGET, "🌍️ Syncing checkpoints from {from_epoch} for shard group {shard_group}");
            // Perform sync operations using the pool and checkpoint
            let validator_status = self.validator_status.clone();
            let checkpoints: Vec<_> = pool
                .try_with_random_members(|mut session| {
                    let validator_status = validator_status.clone();
                    async move {
                        // Verify how far this peer has committed before trusting it as a sync source.
                        // `probe` only returns Err for a forged/malformed proof (other failures are
                        // logged internally and return Ok(None)), which disqualifies the peer so
                        // another committee member is tried.
                        if let Err(e) = validator_status.probe(&mut session, shard_group).await {
                            return Err(NetworkStateSyncError::InvalidCommitProof {
                                details: format!("shard group {shard_group}: {e}"),
                            });
                        }
                        let resp = session
                            .get_checkpoints(rpc::GetCheckpointsRequest {
                                from_epoch: Some(from_epoch.into()),
                                num_to_return: 100,
                            })
                            .await?;

                        debug!(target: LOG_TARGET, "🌍️ Received {} checkpoints for shard group {} from peer {}", resp.checkpoints.len(), shard_group, session.peer_address());

                        resp.checkpoints
                            .into_iter()
                            .map(|cp| {
                                EpochCheckpoint::try_from(cp).map_err(|e| {
                                    NetworkStateSyncError::InvalidCheckpoint {
                                        details: format!(
                                            "Failed to convert checkpoint for shard group {}: {}",
                                            shard_group, e
                                        ),
                                    }
                                })
                            })
                            .collect()
                    }
                })
                .await?;

            if checkpoints.is_empty() {
                info!(target: LOG_TARGET, "🌍️ No checkpoints found for shard group {shard_group} from epoch {from_epoch} (prev_epoch {prev_epoch})");
                sync_plan_mut.add_checkpoint_sync_progress(shard_group, prev_epoch);
                let sync_progress_snapshot = sync_plan_mut.sync_progress().clone();
                self.store
                    .with_write_tx(move |tx| tx.key_value_set(Key::SyncProgress, sync_progress_snapshot))
                    .await?;
                continue;
            }

            info!(target: LOG_TARGET, "🌍️ Found {} checkpoints for shard group {shard_group} from epoch {from_epoch}", checkpoints.len());

            for checkpoint in checkpoints {
                info!(target: LOG_TARGET, "🌍️ Validating checkpoint for shard group {shard_group}: {}", checkpoint.header().calculate_hash());

                let checkpoint_shard_group =
                    checkpoint
                        .checked_shard_group()
                        .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                            details: format!("Checkpoint for shard group {} is not valid: {}", shard_group, e),
                        })?;

                // TODO: we require historical committees to validate older checkpoints. Figure out the best way to
                //       avoid needing the full historical validator data (e.g. VN merkle inclusion proof + historic L1
                // block MR), or,       decide it is ok to require this data to be locally stored by all
                // indexers. For now, to avoid       complexity that may be removed later, we'll skip
                // validating them and only validate prev_epochs       checkpoint.
                if checkpoint.epoch() == prev_epoch {
                    // Use the checkpoint's own shard group, not the iterator's: the network may have
                    // had a different shard-group structure at prev_epoch than the current epoch we
                    // are iterating, so the QC is signed by the committee for `checkpoint_shard_group`,
                    // not `shard_group`.
                    let committee = self
                        .epoch_manager
                        .get_committee_by_shard_group(checkpoint.epoch(), checkpoint_shard_group)
                        .await?;
                    checkpoint
                        .validate(checkpoint.epoch(), committee.quorum_threshold(), |pk| {
                            Ok(committee.get_power_by_public_key(pk).unwrap_or_else(VotePower::zero))
                        })
                        .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                            details: format!(
                                "Failed to validate checkpoint for shard group {}: {}",
                                checkpoint_shard_group, e
                            ),
                        })?;
                } else {
                    checkpoint
                        .validate_well_formed()
                        .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                            details: format!(
                                "Failed to validate well-formedness of checkpoint for shard group {}: {}",
                                checkpoint_shard_group, e
                            ),
                        })?;
                    debug!(target: LOG_TARGET, "🌍️ Skipping checkpoint for shard group {shard_group} with epoch {} (expected {})", checkpoint.epoch(), prev_epoch);
                }

                info!(target: LOG_TARGET, "🌍️ Inserting checkpoint for {}, shard group {}", checkpoint.epoch(), checkpoint_shard_group);

                self.stats.increment_checkpoints();
                sync_plan_mut.add_checkpoint_sync_progress(shard_group, checkpoint.epoch());
                let xtr_exhausted = Amount::from(checkpoint.header().accumulated_data().total_exhaust_burn);
                let checkpoint_epoch = checkpoint.epoch();
                let sync_progress_snapshot = sync_plan_mut.sync_progress().clone();
                self.store
                    .with_write_tx(move |tx| {
                        if !tx.epoch_checkpoint_exists(shard_group, checkpoint_epoch)? {
                            tx.insert_or_ignore_epoch_checkpoint(&checkpoint)?;

                            let exhausted = tx
                                .key_value_get_value::<_, Amount>(Key::TariAccumulatedExhaustBurn)
                                .optional()?;

                            let new_exhausted = exhausted.unwrap_or_else(Amount::zero) + xtr_exhausted;
                            tx.key_value_set(Key::TariAccumulatedExhaustBurn, new_exhausted)?;
                        }
                        tx.key_value_set(Key::SyncProgress, sync_progress_snapshot)
                    })
                    .await?;
            }
        }

        Ok(())
    }

    async fn sync_state(&mut self, sync_plan_mut: &mut SyncPlan) -> Result<(), NetworkStateSyncError> {
        let committee_pools = sync_plan_mut.committee_pools().clone();

        let mut has_synced_global_shard = false;

        for (shard_group, mut pool) in committee_pools {
            // TODO: consider syncing shards in epoch chunks rather than one after another
            let mut session = match pool.new_session().await {
                Ok(s) => s,
                Err(e) => {
                    warn!(target: LOG_TARGET, "⚠️ Failed to create session for shard group {}: {}. Continuing with others", shard_group, e);
                    continue;
                },
            };
            match self.validator_status.probe(&mut session, shard_group).await {
                Ok(Some(verified_tip)) => {
                    // Record the quorum-signed state root so the read path can skip re-validating
                    // commit proofs for this tip. A failure here must not abort the state sync.
                    if let Err(e) = self.persist_verified_tip(verified_tip).await {
                        warn!(target: LOG_TARGET, "⚠️ Failed to record verified state root for shard group {}: {}", shard_group, e);
                    }
                },
                Ok(None) => {},
                // probe only returns Err for an invalid (forged) commit proof.
                Err(e) => {
                    warn!(target: LOG_TARGET, "⚠️ Validator {} for shard group {} served an INVALID commit proof: {}. Skipping this round.", session.peer_address(), shard_group, e);
                    continue;
                },
            }

            // Every committee holds the global shard, so it is synced once per round from whichever
            // committee is reached first. Shard 0 sorts before every preshard, which keeps the cursor
            // list ascending as the responder requires.
            let shards = (!has_synced_global_shard)
                .then_some(Shard::global())
                .into_iter()
                .chain(shard_group.shard_iter());

            self.sync_shard_group_state(shards, sync_plan_mut, shard_group, &mut session)
                .await?;
            has_synced_global_shard = true;
        }

        Ok(())
    }

    /// Syncs every given shard from `session`, which serves them all over a single stream.
    ///
    /// A shard that has never been synced wants only the current head state rather than its full
    /// history, which is expressed by the `UP_ONLY` filter. Filters apply to the whole request, so
    /// such shards are streamed separately from the ones being caught up incrementally: two streams
    /// per shard group at most, and one in the steady state.
    async fn sync_shard_group_state(
        &mut self,
        shards: impl Iterator<Item = Shard>,
        sync_plan_mut: &mut SyncPlan,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
    ) -> Result<(), NetworkStateSyncError> {
        let value_filters = SubstateValueFilterFlags::UTXO |
            SubstateValueFilterFlags::VALIDATOR_FEE_POOL |
            SubstateValueFilterFlags::CLAIMED_OUTPUT_TOMBSTONE |
            SubstateValueFilterFlags::TRANSACTION_RECEIPT |
            SubstateValueFilterFlags::TEMPLATE_METADATA;

        let mut from_scratch = Vec::new();
        let mut incremental = Vec::new();
        for shard in shards {
            let prev_version = sync_plan_mut
                .sync_progress()
                .last_state_versions
                .get(&shard)
                .map_or(0, |(v, _)| v.as_u64());
            let cursor = rpc::ShardCursor {
                shard: shard.as_u32(),
                start_state_version: prev_version + 1,
            };
            if prev_version == 0 {
                from_scratch.push(cursor);
            } else {
                incremental.push(cursor);
            }
        }

        if !from_scratch.is_empty() {
            info!(
                target: LOG_TARGET,
                "🌍️ Syncing {} shard(s) in shard group {shard_group} from scratch. Only fetching the head state.",
                from_scratch.len()
            );
            self.stream_shard_state(
                from_scratch,
                value_filters | SubstateValueFilterFlags::UP_ONLY,
                sync_plan_mut,
                shard_group,
                session,
            )
            .await?;
        }

        if !incremental.is_empty() {
            self.stream_shard_state(incremental, value_filters, sync_plan_mut, shard_group, session)
                .await?;
        }

        Ok(())
    }

    /// Records a committee-validated tip into the verified-root store, after a fail-open epoch
    /// continuity check: the committee's quorum-signed `epoch_hash` must match the epoch hash the
    /// indexer independently derives from the base layer. A mismatch is logged loudly but does not
    /// stop the tip being recorded - the read path is sound regardless, so this is anomaly detection
    /// (forged checkpoint / L1 reorg), not a gate.
    async fn persist_verified_tip(&self, tip: VerifiedBlockTip) -> Result<(), NetworkStateSyncError> {
        match self.epoch_manager.get_epoch_hash(tip.epoch).await {
            Ok(expected) if expected != tip.epoch_hash => {
                error!(
                    target: LOG_TARGET,
                    "⚠️ Epoch continuity mismatch for {} epoch {}: committee epoch_hash {} != base-layer-derived {}. Recording tip anyway.",
                    tip.shard_group, tip.epoch, tip.epoch_hash, expected
                );
            },
            Ok(_) => {},
            Err(e) => {
                // Not yet resolvable (e.g. the epoch just changed); skip the check and retry next round.
                debug!(target: LOG_TARGET, "Epoch hash for epoch {} unavailable for continuity check: {e}", tip.epoch);
            },
        }

        let root = VerifiedStateRoot::from_verified_tip(&tip);
        self.store
            .with_write_tx(move |tx| tx.upsert_verified_state_root(&root))
            .await?;
        Ok(())
    }

    /// Consumes a single `sync_state` stream covering `cursors`.
    ///
    /// The responder streams each shard's updates contiguously and closes it off with a completion
    /// marker, so progress is recorded per shard as the stream advances - an interrupted stream keeps
    /// everything already committed and simply resumes from the recorded cursors next round.
    #[expect(clippy::too_many_lines)]
    async fn stream_shard_state(
        &mut self,
        cursors: Vec<rpc::ShardCursor>,
        value_filters: SubstateValueFilterFlags,
        sync_plan_mut: &mut SyncPlan,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
    ) -> Result<(), NetworkStateSyncError> {
        let mut order = StreamOrder::new(&cursors);

        info!(
            target: LOG_TARGET,
            "🌍️ Starting state sync for {} shard(s) in shard group {shard_group} from peer {}",
            cursors.len(),
            session.peer_address()
        );

        let mut stream = session
            .sync_state(rpc::SyncStateRequest {
                cursors,
                // Sync to latest epoch
                until_epoch: None,
                value_filters: value_filters.bits(),
            })
            .await?;

        // Buffers accumulate a single (shard, state version) at a time: the responder splits an
        // oversized version into chunks flagged `has_more`, and the last chunk flushes them.
        let mut update_buf = Vec::new();
        let mut utxos_buf = Vec::new();
        let mut transactions_buf = Vec::new();
        let mut validator_fee_pools_buf = Vec::new();
        let mut template_catalogue_buf: Vec<(TemplateAddress, PublishedTemplateMetadata)> = Vec::new();
        let mut xtr_claimed = Amount::zero();
        let mut xtr_fees = Amount::zero();
        let mut xtr_receipt_burn = Amount::zero();

        let mut saw_final = false;
        while let Some(result) = stream.next().await {
            let msg = result?;
            let batch = match msg.response {
                Some(rpc::sync_state_response::Response::Batch(batch)) => batch,
                Some(rpc::sync_state_response::Response::Complete(complete)) => {
                    let shard = Shard::from(complete.shard);
                    order
                        .accept_marker(shard)
                        .map_err(|details| NetworkStateSyncError::InvalidStateUpdate { details })?;
                    // Terminal watermark: advance recorded progress to the version the producer is
                    // synced to. This covers trailing versions that streamed no updates because their
                    // substates are all filtered out for our subscription - without it we could never
                    // observe that we have caught up to such a shard and would re-sync it from scratch
                    // every round.
                    let synced_to = StateVersion::new(complete.synced_to_version);
                    let msg_epoch =
                        complete
                            .epoch
                            .map(Epoch::from)
                            .ok_or_else(|| NetworkStateSyncError::InvalidStateUpdate {
                                details: "Received sync completion without epoch".to_string(),
                            })?;
                    // Only persist when the watermark advances - a caught-up shard re-sends the same
                    // version every round, and we must not write on every empty round.
                    let already_synced = sync_plan_mut
                        .sync_progress()
                        .last_state_versions
                        .get(&shard)
                        .is_some_and(|(v, _)| synced_to <= *v);
                    if !already_synced {
                        sync_plan_mut.add_state_sync_progress(shard, synced_to, msg_epoch);
                        let sync_progress_snapshot = sync_plan_mut.sync_progress().clone();
                        self.store
                            .clone()
                            .with_write_tx(move |tx| tx.key_value_set(Key::SyncProgress, sync_progress_snapshot))
                            .await?;
                    }
                    debug!(target: LOG_TARGET, "🌍️ Completed state sync for shard {shard} in shard group {shard_group} to epoch {msg_epoch} and state version {synced_to}");
                    if complete.is_final {
                        saw_final = true;
                        break;
                    }
                    continue;
                },
                None => {
                    return Err(NetworkStateSyncError::InvalidStateUpdate {
                        details: "Received sync state response with no variant set".to_string(),
                    });
                },
            };

            let shard = Shard::from(batch.shard);
            let state_version = StateVersion::new(batch.state_version);
            order
                .accept_batch(shard, state_version, batch.has_more)
                .map_err(|details| NetworkStateSyncError::InvalidStateUpdate { details })?;
            let msg_epoch = batch
                .epoch
                .map(Epoch::from)
                .ok_or_else(|| NetworkStateSyncError::InvalidStateUpdate {
                    details: "Received state update without epoch".to_string(),
                })?;

            for update in batch.updates {
                let update =
                    SubstateUpdateProof::try_from(update).map_err(|e| NetworkStateSyncError::InvalidStateUpdate {
                        details: format!("Failed to convert substate update: {}", e),
                    })?;

                extend_bufs_from_substate_update(
                    &self.notify,
                    shard,
                    state_version,
                    update,
                    msg_epoch,
                    &mut update_buf,
                    &mut utxos_buf,
                    &mut transactions_buf,
                    &mut validator_fee_pools_buf,
                    &mut template_catalogue_buf,
                    &mut xtr_claimed,
                    &mut xtr_fees,
                    &mut xtr_receipt_burn,
                )?;
            }
            if batch.has_more {
                debug!(target: LOG_TARGET, "🌍️ more updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})");
                continue;
            }

            debug!(target: LOG_TARGET, "🌍️ Received {} updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})", update_buf.len());

            self.stats.increase_state_updates(update_buf.len());

            let updates = std::mem::take(&mut update_buf);
            let utxos = std::mem::take(&mut utxos_buf);
            let transactions = std::mem::take(&mut transactions_buf);

            // Upping a substate is the point at which its previous version goes down, so this is the
            // substate cache's only signal that an entry it holds has been superseded.
            self.substate_versions.record_up_all(
                transactions
                    .iter()
                    .flat_map(|(_, receipt)| receipt.diff_summary().upped.iter())
                    .map(|up| (&up.substate_id, up.version)),
            );
            let validator_fee_pools = std::mem::take(&mut validator_fee_pools_buf);
            let template_catalogue = std::mem::take(&mut template_catalogue_buf);

            let updates_len = updates.len();
            let utxos_len = utxos.len();
            let transactions_len = transactions.len();
            let template_catalogue_len = template_catalogue.len();
            let event_count: usize = transactions.iter().map(|(_, t)| t.events.len()).sum();
            self.stats.increase_events(event_count);

            sync_plan_mut.add_state_sync_progress(shard, state_version, msg_epoch);
            let sync_progress_snapshot = sync_plan_mut.sync_progress().clone();

            let network = self.network;
            let event_filters = self.config.event_filters.clone();
            let watched_templates = self.config.watched_templates.clone();
            let xtr_claimed_snapshot = xtr_claimed;
            let xtr_fees_snapshot = xtr_fees;
            let xtr_receipt_burn_snapshot = xtr_receipt_burn;

            let inserted_events = self
                .store
                .clone()
                .with_write_tx(move |tx| -> Result<Vec<InsertedEvent>, StorageError> {
                    debug!(target: LOG_TARGET, "✅ Committing {} updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})", updates_len);
                    // TODO: this is not currently used. Consider removing.
                    tx.batch_insert_substate_transitions(network, shard, state_version, updates)?;
                    debug!(target: LOG_TARGET, "✅ Committing {} UTXOs for shard {shard} (epoch: {msg_epoch})", utxos_len);
                    tx.batch_insert_utxo_updates(msg_epoch, utxos)?;
                    for substate_data in validator_fee_pools {
                        tx.upsert_substate(&substate_data)?;
                    }
                    debug!(target: LOG_TARGET, "✅ Committing {} transactions for shard {shard} (epoch: {msg_epoch})", transactions_len);
                    let inserted = tx.batch_insert_transaction_receipts(transactions, &event_filters)?;
                    if !watched_templates.is_empty() {
                        process_watched_substate_events(tx, &inserted, &watched_templates)?;
                    }

                    if !template_catalogue.is_empty() {
                        debug!(target: LOG_TARGET, "✅ Upserting {} template catalogue entries for shard {shard} (epoch: {msg_epoch})", template_catalogue_len);
                        for (template_addr, metadata) in template_catalogue {
                            tx.upsert_template_catalogue(&template_addr, &metadata)?;
                        }
                    }

                    tx.key_value_set(Key::SyncProgress, sync_progress_snapshot)?;
                    let claimed = tx.key_value_get_value(Key::TariAccumulatedClaimed).optional()?;
                    let new_claimed = claimed.unwrap_or_else(Amount::zero) + xtr_claimed_snapshot;
                    tx.key_value_set(Key::TariAccumulatedClaimed, new_claimed)?;
                    let fees = tx.key_value_get_value(Key::TariAccumulatedFees).optional()?;
                    let new_fees = fees.unwrap_or_else(Amount::zero) + xtr_fees_snapshot;
                    tx.key_value_set(Key::TariAccumulatedFees, new_fees)?;
                    let receipt_burn = tx.key_value_get_value(Key::TariAccumulatedReceiptExhaustBurn).optional()?;
                    let new_receipt_burn = receipt_burn.unwrap_or_else(Amount::zero) + xtr_receipt_burn_snapshot;
                    tx.key_value_set(Key::TariAccumulatedReceiptExhaustBurn, new_receipt_burn)?;
                    Ok(inserted)
                })
                .await?;

            // The stream flushes (has_more == false) once per state version, so each commit must fold only
            // that version's delta. Reset the running totals here, mirroring the buffer drains above.
            xtr_claimed = Amount::zero();
            xtr_fees = Amount::zero();
            xtr_receipt_burn = Amount::zero();

            for inserted in inserted_events {
                self.transaction_event_notify.notify(TransactionEvent {
                    id: inserted.id,
                    transaction_id: inserted.transaction_id,
                    event: inserted.event,
                });
            }
        }

        if !saw_final {
            return Err(NetworkStateSyncError::InvalidStateUpdate {
                details: format!(
                    "State sync stream for shard group {shard_group} ended without a final completion marker"
                ),
            });
        }

        Ok(())
    }
}

/// Enforces the ordering a `sync_state` stream promises across the shards it carries: each shard is
/// streamed contiguously, its versions strictly advance, and nothing for it follows its completion
/// marker.
///
/// The consumer relies on all three. Its buffers hold one `(shard, state version)` at a time, so an
/// interleaved shard would mix two shards' updates into one commit; and because the running economic
/// totals are read-modify-write, re-applying a version already committed would double-count it.
struct StreamOrder {
    /// Highest version committed per requested shard, seeded from the cursor so a responder cannot
    /// replay versions the caller already holds.
    committed_versions: HashMap<Shard, StateVersion>,
    completed: HashSet<Shard>,
    /// Set while a version is split across chunks, until the chunk that flushes it.
    pending_chunk: Option<(Shard, StateVersion)>,
}

impl StreamOrder {
    fn new(cursors: &[rpc::ShardCursor]) -> Self {
        Self {
            committed_versions: cursors
                .iter()
                .map(|c| {
                    (
                        Shard::from(c.shard),
                        StateVersion::new(c.start_state_version.saturating_sub(1)),
                    )
                })
                .collect(),
            completed: HashSet::new(),
            pending_chunk: None,
        }
    }

    fn accept_batch(&mut self, shard: Shard, state_version: StateVersion, has_more: bool) -> Result<(), String> {
        let Some(committed_version) = self.committed_versions.get(&shard).copied() else {
            return Err(format!("Received batch for unrequested shard {shard}"));
        };
        if self.completed.contains(&shard) {
            return Err(format!("Received batch for shard {shard} after its completion marker"));
        }
        if state_version <= committed_version {
            return Err(format!(
                "Received v{state_version} for shard {shard}, which is not ahead of the committed v{committed_version}"
            ));
        }
        if let Some(pending) = self.pending_chunk &&
            pending != (shard, state_version)
        {
            return Err(format!(
                "Received v{state_version} of shard {shard} while v{} of shard {} is still incomplete",
                pending.1, pending.0
            ));
        }

        if has_more {
            self.pending_chunk = Some((shard, state_version));
        } else {
            self.pending_chunk = None;
            self.committed_versions.insert(shard, state_version);
        }
        Ok(())
    }

    fn accept_marker(&mut self, shard: Shard) -> Result<(), String> {
        if !self.committed_versions.contains_key(&shard) {
            return Err(format!("Received completion marker for unrequested shard {shard}"));
        }
        if let Some((pending_shard, pending_version)) = self.pending_chunk {
            return Err(format!(
                "Received completion marker for shard {shard} while v{pending_version} of shard {pending_shard} is \
                 still incomplete"
            ));
        }
        if !self.completed.insert(shard) {
            return Err(format!("Received a second completion marker for shard {shard}"));
        }
        Ok(())
    }
}

fn process_watched_substate_events(
    tx: &mut SqliteStoreWriteTransaction<'_>,
    events: &[InsertedEvent],
    watched_templates: &HashSet<TemplateAddress>,
) -> Result<(), StorageError> {
    use crate::store::IndexerStoreWriteTransaction;

    for inserted in events {
        let event = &inserted.event;
        match event.topic() {
            "std.component.created" => {
                if watched_templates.contains(event.template_address()) &&
                    let Some(substate_id) = event.substate_id()
                {
                    debug!(
                        target: LOG_TARGET,
                        "📌 Watched component created: {} (template: {})",
                        substate_id,
                        event.template_address()
                    );
                    tx.insert_watched_substate(substate_id, event.template_address())?;
                }
            },
            "std.component.template_update" => {
                if let Some(substate_id) = event.substate_id() {
                    let prev_template = event
                        .payload()
                        .get("prev_template")
                        .and_then(|v| TemplateAddress::from_hex(v).ok());

                    let prev_was_watched = prev_template.as_ref().is_some_and(|t| watched_templates.contains(t));
                    let new_is_watched = watched_templates.contains(event.template_address());

                    if prev_was_watched && !new_is_watched {
                        debug!(
                            target: LOG_TARGET,
                            "📌 Watched component removed (template update): {}",
                            substate_id
                        );
                        tx.delete_watched_substate(substate_id)?;
                    } else if new_is_watched {
                        debug!(
                            target: LOG_TARGET,
                            "📌 Watched component updated: {} (template: {})",
                            substate_id,
                            event.template_address()
                        );
                        tx.insert_watched_substate(substate_id, event.template_address())?;
                    } else {
                        // N/A
                    }
                }
            },
            _ => {},
        }
    }
    Ok(())
}

fn extend_bufs_from_substate_update(
    notify: &Notify<IndexerEvent>,
    shard: Shard,
    state_version: StateVersion,
    update: SubstateUpdateProof,
    msg_epoch: Epoch,
    update_buf: &mut Vec<(Epoch, SubstateUpdateProof)>,
    utxos_buf: &mut Vec<UtxoUpdateRecord>,
    transactions_buf: &mut Vec<(TransactionReceiptAddress, TransactionReceipt)>,
    validator_fee_pools_buf: &mut Vec<SubstateData>,
    template_catalogue_buf: &mut Vec<(TemplateAddress, PublishedTemplateMetadata)>,
    xtr_claimed_mut: &mut Amount,
    xtr_fees_mut: &mut Amount,
    xtr_receipt_burn_mut: &mut Amount,
) -> Result<(), NetworkStateSyncError> {
    match &update {
        SubstateUpdateProof::Create(create) => {
            if create.substate.substate_id().is_template() {
                if let Some(metadata) = &create.substate.template_metadata &&
                    let Some(template_addr) = create.substate.substate_id().as_template()
                {
                    template_catalogue_buf.push((template_addr.as_template_address(), metadata.clone()));
                }
                update_buf.push((msg_epoch, update));
                return Ok(());
            }
            match create.substate.value().value() {
                Some(SubstateValue::Utxo(utxo)) => {
                    if let Some(address) = create.substate.substate_id().as_utxo_address() {
                        let is_frozen = utxo.is_frozen();
                        if let Some(ref output) = utxo.output {
                            utxos_buf.push(UtxoUpdateRecord::Unspent(Box::new(UtxoUnspent {
                                address,
                                version: update.version(),
                                shard,
                                state_version,
                                utxo_output: output.clone(),
                                is_frozen,
                            })));
                        }
                    } else {
                        warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received UTXO substate with invalid address: {}", create.substate.substate_id());
                    };
                },
                Some(SubstateValue::TransactionReceipt(receipt)) => {
                    if let Some(address) = update.substate_id().as_transaction_receipt_address() {
                        // Accumulate the realized-rate pair from the same receipt: `burn` is the exhaust
                        // surcharge and `F` (fees minus burn) is the base it was charged on, so
                        // `burn / F` recovers the rate independent of the header-sourced burn total.
                        let fee_receipt = receipt.fee_receipt();
                        let burn = fee_receipt.exhaust_burn_charged();
                        *xtr_receipt_burn_mut += Amount::from(burn);
                        *xtr_fees_mut += Amount::from(fee_receipt.total_fees_charged().saturating_sub(burn));

                        notify.notify(TransactionFinalizedEvent {
                            transaction_id: TransactionId::from_receipt_address(address),
                            outcome: receipt.outcome,
                        });
                        transactions_buf.push((address, receipt.clone()));
                    } else {
                        warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received Transaction Receipt substate with invalid address: {}", create.substate.substate_id());
                    }
                },
                Some(SubstateValue::ValidatorFeePool(_)) => {
                    validator_fee_pools_buf.push(SubstateData {
                        substate_id: create.substate.substate_id().clone(),
                        version: create.substate.version,
                        value: create.substate.value().clone(),
                        template_metadata: None,
                    });
                },
                Some(SubstateValue::ClaimedOutputTombstone(claim)) => {
                    *xtr_claimed_mut += Amount::from(claim.value);
                },
                Some(_) => {
                    warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received unexpected substate value for created substate: {}", create.substate.substate_id());
                },
                None => {
                    let id = create.substate.substate_id();
                    if id.is_transaction_receipt() {
                        warn!(target: LOG_TARGET, "⚠️ Received tx receipt {id} update with no value, it may have been pruned and so will not be indexed");
                    }
                    if let Some(addr) = id.as_utxo_address() {
                        debug!(target: LOG_TARGET, "🌍️ Received UTXO substate {addr} creation with no value. Ignoring as this means it is spent later.");
                    }
                },
            }
        },
        SubstateUpdateProof::Destroy(destroy) => match &destroy.substate_id {
            SubstateId::Utxo(address) => {
                utxos_buf.push(UtxoUpdateRecord::Spent(UtxoSpent {
                    address: address.clone(),
                    shard,
                    version: update.version(),
                    state_version,
                }));
            },

            other if other.is_read_only() => {
                warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received destroy for read only substate: {}", destroy.substate_id);
            },
            _ => {},
        },
    }

    update_buf.push((msg_epoch, update));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod stream_order {
        use super::*;

        const S1: Shard = Shard::from_u32(1);
        const S2: Shard = Shard::from_u32(2);

        fn order(cursors: &[(u32, u64)]) -> StreamOrder {
            let cursors = cursors
                .iter()
                .map(|&(shard, start_state_version)| rpc::ShardCursor {
                    shard,
                    start_state_version,
                })
                .collect::<Vec<_>>();
            StreamOrder::new(&cursors)
        }

        fn v(n: u64) -> StateVersion {
            StateVersion::new(n)
        }

        #[test]
        fn it_accepts_shards_streamed_one_after_another() {
            let mut o = order(&[(1, 1), (2, 1)]);
            o.accept_batch(S1, v(1), false).unwrap();
            o.accept_batch(S1, v(4), false).unwrap();
            o.accept_marker(S1).unwrap();
            o.accept_batch(S2, v(7), false).unwrap();
            o.accept_marker(S2).unwrap();
        }

        #[test]
        fn it_accepts_a_version_split_across_chunks() {
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(3), true).unwrap();
            o.accept_batch(S1, v(3), true).unwrap();
            o.accept_batch(S1, v(3), false).unwrap();
            o.accept_marker(S1).unwrap();
        }

        #[test]
        fn it_rejects_a_version_below_the_cursor() {
            // A cursor of 5 asks to resume at v5, so v5 is wanted and everything under it is already held.
            assert!(order(&[(1, 5)]).accept_batch(S1, v(3), false).is_err());
            assert!(order(&[(1, 5)]).accept_batch(S1, v(4), false).is_err());
            order(&[(1, 5)]).accept_batch(S1, v(5), false).unwrap();
        }

        #[test]
        fn it_rejects_a_replayed_version() {
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(9), false).unwrap();
            assert!(o.accept_batch(S1, v(9), false).is_err());
        }

        #[test]
        fn it_rejects_a_regressing_version() {
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(9), false).unwrap();
            assert!(o.accept_batch(S1, v(8), false).is_err());
        }

        #[test]
        fn it_rejects_a_batch_after_the_shards_marker() {
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(2), false).unwrap();
            o.accept_marker(S1).unwrap();
            assert!(o.accept_batch(S1, v(3), false).is_err());
        }

        #[test]
        fn it_rejects_an_interleaved_shard_mid_version() {
            let mut o = order(&[(1, 1), (2, 1)]);
            o.accept_batch(S1, v(3), true).unwrap();
            assert!(o.accept_batch(S2, v(1), false).is_err());
        }

        #[test]
        fn it_rejects_a_marker_while_a_version_is_incomplete() {
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(3), true).unwrap();
            assert!(o.accept_marker(S1).is_err());
        }

        #[test]
        fn it_rejects_a_second_marker_for_a_shard() {
            let mut o = order(&[(1, 1)]);
            o.accept_marker(S1).unwrap();
            assert!(o.accept_marker(S1).is_err());
        }

        #[test]
        fn it_rejects_unrequested_shards() {
            assert!(order(&[(1, 1)]).accept_batch(S2, v(1), false).is_err());
            assert!(order(&[(1, 1)]).accept_marker(S2).is_err());
        }
    }
}
