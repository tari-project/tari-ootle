//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    convert::Infallible,
    pin::pin,
    sync::Arc,
    time::Duration,
};

use futures::{StreamExt, future::Either, stream::FuturesUnordered};
use log::*;
use ootle_network::Network;
use prost::Message;
use tari_engine_types::{
    published_template::PublishedTemplateMetadata,
    substate::{SubstateId, SubstateValue},
    transaction_receipt::TransactionReceipt,
};
use tari_epoch_manager::{
    EpochManagerEvent,
    EpochManagerReader,
    service::{EpochManagerHandle, NetworkDescription},
};
use tari_indexer_client::event::{IndexerEvent, NewEpochEvent, TransactionEvent, TransactionFinalizedEvent};
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
use tari_rpc_framework::RpcRequestOptions;
use tari_shutdown::ShutdownSignal;
use tari_template_lib_types::{Amount, TemplateAddress, TransactionReceiptAddress};
use tokio::{
    sync::{broadcast, watch},
    time,
};
use tokio_util::sync::CancellationToken;

#[cfg(feature = "metrics")]
use crate::{
    exhaust_burn_rate::resolve_exhaust_burn_rate_for_epoch,
    network_state_sync::NetworkStateMetrics,
    store::ReadOnlyStore,
    substate_cache::SubstateCacheMetrics,
    substate_manager::SubstateManager,
};
use crate::{
    network_state_sync::{
        committee_client::{ValidatorCommitteeRpcPool, ValidatorRpcSession},
        config::NetworkWideStateSyncConfig,
        consensus_epoch::ConsensusEpoch,
        error::NetworkStateSyncError,
        shard_watermarks::ShardWatermarks,
        stats::SyncStats,
        sync_plan::SyncPlan,
        sync_progress::{SharedSyncProgress, SyncProgress},
        validator_status::ValidatorStatusMonitor,
    },
    notify::Notify,
    storage_sqlite::{
        SqliteIndexerStore,
        SqliteStoreWriteTransaction,
        models::{Key, SubstateCacheInvalidation, UtxoSpent, UtxoUnspent, UtxoUpdateRecord, VerifiedStateRoot},
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
/// How often a shard group whose committee has not yet been seen committing in the epoch manager's
/// epoch is probed again.
const CONSENSUS_EPOCH_PROBE_INTERVAL: Duration = Duration::from_secs(10);
/// The highest state version accepted from a peer or carried in the recorded progress. State
/// versions are stored in signed 64-bit columns, which must also hold the version just past any
/// recorded one: the next cursor, and the substate cache journals retirements one past a watermark.
const MAX_STATE_VERSION: StateVersion = StateVersion::new(i64::MAX as u64 - 1);
/// The most a peer may stream, in encoded bytes, for one state version before completing it. A
/// version is buffered whole and committed at once, so this bounds the memory a peer can hold to a
/// constant multiple of it: the buffered updates are held decoded, alongside what is derived from
/// them.
const MAX_BUFFERED_VERSION_BYTES: usize = 256 * 1024 * 1024;

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
    shard_watermarks: Arc<ShardWatermarks>,
    consensus_epoch: ConsensusEpoch,
    #[cfg(feature = "metrics")]
    metrics: NetworkStateMetrics,
    #[cfg(feature = "metrics")]
    substate_cache_metrics: SubstateCacheMetrics,
    #[cfg(feature = "metrics")]
    substate_manager: SubstateManager,
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
        shard_watermarks: Arc<ShardWatermarks>,
        consensus_epoch: ConsensusEpoch,
        #[cfg(feature = "metrics")] metrics: NetworkStateMetrics,
        #[cfg(feature = "metrics")] substate_cache_metrics: SubstateCacheMetrics,
        #[cfg(feature = "metrics")] substate_manager: SubstateManager,
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
            shard_watermarks,
            consensus_epoch,
            #[cfg(feature = "metrics")]
            metrics,
            #[cfg(feature = "metrics")]
            substate_cache_metrics,
            #[cfg(feature = "metrics")]
            substate_manager,
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

        let mut report_interval = time::interval(self.config.work_interval);
        report_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        report_interval.reset();

        loop {
            let sync_plan = self.initialize_sync_plan().await?;
            let plan_epoch = sync_plan.network_description().epoch();
            let partition = sync_plan
                .network_description()
                .shard_groups_iter()
                .collect::<BTreeSet<_>>();
            self.consensus_epoch.track_groups(partition.iter().copied());
            let (epoch_tx, epoch_rx) = watch::channel(plan_epoch);
            // A plan is drawn against one partition of the shards into groups. An epoch that keeps
            // the partition is handled by each group on its own: it winds its stream down, syncs its
            // checkpoints, re-resolves its committee and reopens from the cursor it holds. Only a
            // change to the partition itself - which shards each group serves - draws a new plan,
            // and then every stream is wound down first; the cursors survive in the persisted
            // progress, so nothing is re-streamed.
            //
            // Wound down, not dropped: a write transaction runs to its commit on a blocking thread
            // whether or not the future awaiting it survives, and a plan drawn from progress read
            // before that commit would carry a cursor the commit has moved past. Once persisted
            // over the committed one, that cursor re-streams a version whose economic totals were
            // already folded in. So every stream is told to stop, and the plan is awaited to the
            // end before the next is read.
            let cancel = CancellationToken::new();
            let mut sync = pin!(self.clone().sync_plan(sync_plan, cancel.clone(), epoch_rx));
            loop {
                tokio::select! {
                    event = epoch_events.recv() => {
                        // Every way out of here winds the plan down first, for the reason above.
                        let outcome: Result<(), NetworkStateSyncError> = match event {
                            Ok(EpochManagerEvent::EpochChanged { epoch, .. }) => {
                                info!(target: LOG_TARGET, "🌍️ Epoch changed to {}.", epoch);
                                self.notify.notify(NewEpochEvent { epoch });
                                match self.epoch_manager.get_network_description().await {
                                    Ok(network_desc) if plan_absorbs_epoch(plan_epoch, &partition, &network_desc) => {
                                        let epoch = network_desc.epoch();
                                        epoch_tx.send_if_modified(|current| {
                                            let moved = *current != epoch;
                                            *current = epoch;
                                            moved
                                        });
                                        continue;
                                    },
                                    Ok(_) => {
                                        info!(target: LOG_TARGET, "🌍️ Re-planning the state sync at epoch {}", epoch);
                                        Ok(())
                                    },
                                    // The plan is re-drawn from a fresh description a work interval
                                    // later rather than every stream being torn down for good.
                                    Err(err) => {
                                        warn!(target: LOG_TARGET, "⚠️ Failed to read the network description at epoch {}: {}. Re-planning the state sync", epoch, err);
                                        Ok(())
                                    },
                                }
                            },
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!(target: LOG_TARGET, "⚠️ Missed {n} epoch event(s). Re-planning the state sync");
                                Ok(())
                            },
                            Err(broadcast::error::RecvError::Closed) => Err(NetworkStateSyncError::InvariantError {
                                details: "Epoch manager stopped publishing events".to_string(),
                            }),
                        };
                        cancel.cancel();
                        sync.await?;
                        outcome?;
                        break;
                    },
                    result = &mut sync => {
                        result?;
                        time::sleep(self.config.work_interval).await;
                        break;
                    },
                    _ = report_interval.tick() => {
                        self.stats.log_stats();
                        self.stats.reset();
                        #[cfg(feature = "metrics")]
                        self.update_metrics().await;
                    },
                }
            }
        }
    }

    /// Reads the persisted economic totals and publishes them to the Prometheus gauges. Metrics are
    /// observability only, so a read failure is logged rather than propagated into the sync loop.
    #[cfg(feature = "metrics")]
    async fn update_metrics(&self) {
        match ReadOnlyStore::new(self.store.clone()).get_tari_economics().await {
            Ok(economics) => {
                let current_epoch = self.epoch_manager.get_current_epoch();
                let target_burn_rate_bps =
                    resolve_exhaust_burn_rate_for_epoch(&self.substate_manager, self.network, current_epoch)
                        .await
                        .as_bps();
                self.metrics.update(&economics, target_burn_rate_bps);
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "⚠️ Failed to update network economics metrics: {err}");
            },
        }
    }

    async fn initialize_sync_plan(&self) -> Result<SyncPlan, NetworkStateSyncError> {
        let network_desc = self.epoch_manager.get_network_description().await?;
        let current_epoch = network_desc.epoch();
        let (sync_progress, rewound) = self
            .store
            .with_read_tx(move |tx| {
                let mut progress = tx
                    .key_value_get_value::<_, SyncProgress>(Key::SyncProgress)
                    .optional()?
                    .unwrap_or_default();
                let rewound = rewind_out_of_range_progress(tx, &mut progress, next_epoch(current_epoch))?;
                Ok::<_, StorageError>((progress, rewound))
            })
            .await?;
        if !rewound.is_empty() {
            log_rewound(&sync_progress, &rewound, "Recorded sync progress is out of range");
            let snapshot = sync_progress.clone();
            self.store
                .with_write_tx(move |tx| tx.key_value_set(Key::SyncProgress, snapshot))
                .await?;
        }

        let mut committee_pools = HashMap::with_capacity(network_desc.num_committees());
        for shard_group in network_desc.shard_groups_iter() {
            let pool = ValidatorCommitteeRpcPool::new(shard_group, self.networking.clone(), self.epoch_manager.clone());
            committee_pools.insert(shard_group, pool);
        }

        Ok(SyncPlan::new(
            network_desc,
            SharedSyncProgress::new(sync_progress),
            committee_pools,
        ))
    }

    /// Follows every shard group's tip until `cancel` is triggered, at which point it returns once
    /// every stream has stopped between messages. `epoch` carries the epoch each group is to serve;
    /// a group answers a change to it on its own. Returns early only on an error that is not one
    /// shard group's alone.
    async fn sync_plan(
        self,
        sync_plan: SyncPlan,
        cancel: CancellationToken,
        epoch: watch::Receiver<Epoch>,
    ) -> Result<(), NetworkStateSyncError> {
        if sync_plan.network_description().epoch.is_zero() {
            info!(target: LOG_TARGET, "🌍️ Current epoch is zero, nothing to sync.");
            cancel.cancelled().await;
            return Ok(());
        }
        info!(target: LOG_TARGET, "🌍️ Starting network-wide state sync...");
        self.follow_state(&sync_plan, &cancel, &epoch).await
    }

    /// Syncs `shard_group`'s checkpoints up to the epoch before `epoch`, from wherever it left off.
    /// Nothing to do once they are recorded, so this is run before every stream the group opens.
    ///
    /// Recorded progress is reconciled against the validated checkpoint: a shard recorded ahead of
    /// the version its committee committed to by then is rewound and its watermark withdrawn.
    #[expect(clippy::too_many_lines)]
    async fn sync_group_checkpoints(
        &self,
        shard_group: ShardGroup,
        syncs_global_shard: bool,
        pool: &mut ValidatorCommitteeRpcPool,
        epoch: Epoch,
        progress: &SharedSyncProgress,
    ) -> Result<(), NetworkStateSyncError> {
        let Some(prev_epoch) = epoch.checked_sub(Epoch(1)) else {
            return Ok(());
        };
        let from_epoch = progress
            .lock()
            .await
            .checkpoint_epoch(shard_group)
            .unwrap_or_else(Epoch::zero);
        if from_epoch >= prev_epoch {
            debug!(target: LOG_TARGET, "🌍️ No checkpoints to sync for shard group {shard_group} from epoch {from_epoch}");
            return Ok(());
        }
        info!(target: LOG_TARGET, "🌍️ Syncing checkpoints from {from_epoch} for shard group {shard_group}");
        // Perform sync operations using the pool and checkpoint
        let validator_status = self.validator_status.clone();
        let consensus_epoch = self.consensus_epoch.clone();
        let checkpoints: Vec<_> = pool
            .try_with_random_members(|mut session| {
                let validator_status = validator_status.clone();
                let consensus_epoch = consensus_epoch.clone();
                async move {
                    // Verify how far this peer has committed before trusting it as a sync source.
                    // `probe` only returns Err for a forged/malformed proof (other failures are
                    // logged internally and return Ok(None)), which disqualifies the peer so
                    // another committee member is tried.
                    match validator_status.probe(&mut session, shard_group).await {
                        Ok(Some(tip)) => consensus_epoch.observe(tip.shard_group, tip.epoch),
                        Ok(None) => {},
                        Err(e) => {
                            return Err(NetworkStateSyncError::InvalidCommitProof {
                                details: format!("shard group {shard_group}: {e}"),
                            });
                        },
                    }
                    let resp = session
                        .get_checkpoints(rpc::GetCheckpointsRequest {
                            from_epoch: Some(from_epoch.into()),
                            num_to_return: 100,
                            shard_group: None,
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
            let mut progress = progress.lock().await;
            progress.record_checkpoint(shard_group, prev_epoch);
            let sync_progress_snapshot = progress.clone();
            self.store
                .with_write_tx(move |tx| tx.key_value_set(Key::SyncProgress, sync_progress_snapshot))
                .await?;
            return Ok(());
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

            // Only a quorum-validated checkpoint may rewind progress. The global shard is streamed
            // from one committee only, so only that committee's checkpoint speaks for it.
            let checkpoint_versions = (checkpoint.epoch() == prev_epoch).then(|| {
                let reconciles_global = syncs_global_shard && checkpoint_shard_group == shard_group;
                reconciles_global
                    .then_some(Shard::global())
                    .into_iter()
                    .chain(checkpoint_shard_group.shard_iter())
                    .map(|shard| (shard, StateVersion::new(checkpoint.get_shard_state_version(shard))))
                    .collect::<Vec<_>>()
            });

            self.stats.increment_checkpoints();
            let xtr_exhausted = Amount::from(checkpoint.header().accumulated_data().total_exhaust_burn);
            let checkpoint_epoch = checkpoint.epoch();
            let mut progress = progress.lock().await;
            let mut sync_progress_snapshot = progress.clone();
            sync_progress_snapshot.record_checkpoint(shard_group, checkpoint_epoch);
            let (sync_progress_snapshot, rewound) = self
                .store
                .with_write_tx(move |tx| {
                    let rewound = match checkpoint_versions {
                        Some(versions) => reconcile_progress_with_checkpoint(
                            &mut **tx,
                            &mut sync_progress_snapshot,
                            checkpoint_epoch,
                            versions,
                        )?,
                        None => Vec::new(),
                    };
                    if !tx.epoch_checkpoint_exists(shard_group, checkpoint_epoch)? {
                        tx.insert_or_ignore_epoch_checkpoint(&checkpoint)?;

                        let exhausted = tx
                            .key_value_get_value::<_, Amount>(Key::TariAccumulatedExhaustBurn)
                            .optional()?;

                        let new_exhausted = exhausted.unwrap_or_else(Amount::zero) + xtr_exhausted;
                        tx.key_value_set(Key::TariAccumulatedExhaustBurn, new_exhausted)?;
                    }
                    tx.key_value_set(Key::SyncProgress, &sync_progress_snapshot)?;
                    Ok::<_, StorageError>((sync_progress_snapshot, rewound))
                })
                .await?;
            *progress = sync_progress_snapshot;
            if !rewound.is_empty() {
                log_rewound(
                    &progress,
                    &rewound,
                    &format!("Recorded sync progress is ahead of the validated {checkpoint_epoch} checkpoint"),
                );
                for shard in &rewound {
                    self.shard_watermarks.forget(*shard);
                }
            }
        }
        Ok(())
    }

    /// Follows every shard group's tip at once, each on its own stream, until cancelled. Returns
    /// early only on an error that is not one shard group's alone; a group whose peer fails is
    /// retried on its own without disturbing the others.
    async fn follow_state(
        &self,
        sync_plan: &SyncPlan,
        cancel: &CancellationToken,
        epoch: &watch::Receiver<Epoch>,
    ) -> Result<(), NetworkStateSyncError> {
        let mut committee_pools = sync_plan.committee_pools().iter().collect::<Vec<_>>();
        committee_pools.sort_by_key(|(shard_group, _)| **shard_group);

        // Every committee holds the global shard, so it is claimed by exactly one group: the lowest.
        let mut groups = committee_pools
            .into_iter()
            .enumerate()
            .map(|(i, (shard_group, pool))| {
                self.clone().follow_shard_group(
                    *shard_group,
                    pool.clone(),
                    i == 0,
                    sync_plan.sync_progress().clone(),
                    cancel.clone(),
                    epoch.clone(),
                )
            })
            .collect::<FuturesUnordered<_>>();

        let followed = async {
            while let Some(result) = groups.next().await {
                result?;
            }
            Ok(())
        };
        // The groups wind down on `cancel`; the probe loop is dropped once they have.
        tokio::select! {
            result = followed => result,
            never = self.track_consensus_epoch(sync_plan, epoch.clone()) => match never {},
        }
    }

    /// Probes each shard group whose committee has not been seen committing in the epoch manager's
    /// epoch, every [`CONSENSUS_EPOCH_PROBE_INTERVAL`], so that [`ConsensusEpoch`] follows a committee
    /// into a new epoch soon after its end-of-epoch block. A followed stream only probes when it
    /// reopens, which on a quiet network is up to a stream deadline away.
    async fn track_consensus_epoch(&self, sync_plan: &SyncPlan, epoch: watch::Receiver<Epoch>) -> Infallible {
        let mut pools = sync_plan.committee_pools().clone();
        let mut interval = time::interval(CONSENSUS_EPOCH_PROBE_INTERVAL);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let target = *epoch.borrow();
            for shard_group in self.consensus_epoch.groups_behind(target) {
                let Some(pool) = pools.get_mut(&shard_group) else {
                    continue;
                };
                let mut session = match pool.new_session().await {
                    Ok(session) => session,
                    Err(e) => {
                        debug!(target: LOG_TARGET, "No session to probe the consensus epoch of shard group {shard_group}: {e}");
                        continue;
                    },
                };
                match self.validator_status.probe(&mut session, shard_group).await {
                    Ok(Some(tip)) => self.consensus_epoch.observe(tip.shard_group, tip.epoch),
                    Ok(None) => {},
                    Err(e) => {
                        warn!(target: LOG_TARGET, "⚠️ Validator {} for shard group {} served an INVALID commit proof: {}", session.peer_address(), shard_group, e);
                    },
                }
            }
        }
    }

    /// Keeps one shard group synced: syncs its checkpoints, opens a stream from a committee member,
    /// follows it until it ends, and goes round again, until cancelled.
    ///
    /// A stream that ends because the validator had nothing to send for the deadline is reopened at
    /// once - that is the ordinary end of a followed stream, and the reopen refreshes each shard's
    /// watermark. So is one wound down because the epoch moved: the next round syncs the new
    /// checkpoints and resolves the committee at the new epoch. One closed by a validator that does
    /// not follow, or failed by the peer, waits the work interval first: the former is polling, the
    /// latter wants a different peer.
    async fn follow_shard_group(
        mut self,
        shard_group: ShardGroup,
        mut pool: ValidatorCommitteeRpcPool,
        syncs_global_shard: bool,
        progress: SharedSyncProgress,
        cancel: CancellationToken,
        mut epoch: watch::Receiver<Epoch>,
    ) -> Result<(), NetworkStateSyncError> {
        while !cancel.is_cancelled() {
            let current_epoch = *epoch.borrow_and_update();
            if let Err(err) = self
                .sync_group_checkpoints(shard_group, syncs_global_shard, &mut pool, current_epoch, &progress)
                .await
            {
                if !err.is_peer_fault() {
                    return Err(err);
                }
                warn!(target: LOG_TARGET, "⚠️ Checkpoint sync for shard group {} failed: {}", shard_group, err);
                self.pause(shard_group, "checkpoint sync failed", &cancel, &mut epoch)
                    .await;
                continue;
            }
            let mut session = match pool.new_session().await {
                Ok(s) => s,
                Err(e) => {
                    warn!(target: LOG_TARGET, "⚠️ Failed to create session for shard group {}: {}", shard_group, e);
                    self.pause(shard_group, "no session", &cancel, &mut epoch).await;
                    continue;
                },
            };
            match self.validator_status.probe(&mut session, shard_group).await {
                Ok(Some(verified_tip)) => {
                    self.consensus_epoch
                        .observe(verified_tip.shard_group, verified_tip.epoch);
                    // Record the quorum-signed state root so the read path can skip re-validating
                    // commit proofs for this tip. A failure here must not abort the state sync.
                    if let Err(e) = self.persist_verified_tip(verified_tip).await {
                        warn!(target: LOG_TARGET, "⚠️ Failed to record verified state root for shard group {}: {}", shard_group, e);
                    }
                },
                Ok(None) => {},
                // probe only returns Err for an invalid (forged) commit proof.
                Err(e) => {
                    warn!(target: LOG_TARGET, "⚠️ Validator {} for shard group {} served an INVALID commit proof: {}", session.peer_address(), shard_group, e);
                    self.pause(shard_group, "invalid commit proof", &cancel, &mut epoch)
                        .await;
                    continue;
                },
            }

            // Shard 0 sorts before every preshard, which keeps the cursor list ascending as the
            // responder requires.
            let shards = syncs_global_shard
                .then_some(Shard::global())
                .into_iter()
                .chain(shard_group.shard_iter());

            // A responder that cannot serve these shards - it left the committee, or the epoch this
            // indexer resolved its committees at has moved on - costs this shard group a retry and
            // no more.
            match self
                .sync_shard_group_state(shards, &progress, shard_group, &mut session, &cancel, &mut epoch)
                .await
            {
                Ok(StreamEnd::Cancelled) => return Ok(()),
                Ok(StreamEnd::TimedOut) => {
                    debug!(target: LOG_TARGET, "🌍️ State sync stream for shard group {shard_group} from {} had nothing to send for the deadline. Reopening", session.peer_address());
                },
                Ok(StreamEnd::EpochAdvanced) => {
                    info!(target: LOG_TARGET, "🌍️ Epoch advanced to {}. Reopening state sync for shard group {shard_group}", *epoch.borrow());
                },
                Ok(StreamEnd::Final) => {
                    self.pause(
                        shard_group,
                        "the validator closed the stream at its tip and does not follow",
                        &cancel,
                        &mut epoch,
                    )
                    .await;
                },
                Err(err) if err.is_peer_fault() => {
                    warn!(target: LOG_TARGET, "⚠️ State sync for shard group {} from {} failed: {}", shard_group, session.peer_address(), err);
                    self.pause(shard_group, "the peer failed", &cancel, &mut epoch).await;
                },
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    /// Waits the work interval before a shard group tries again, or less if the plan is wound down
    /// or the epoch moves - the new epoch wants its checkpoints synced and its committee resolved
    /// before anything is retried.
    async fn pause(
        &self,
        shard_group: ShardGroup,
        reason: &str,
        cancel: &CancellationToken,
        epoch: &mut watch::Receiver<Epoch>,
    ) {
        let interval = self.config.work_interval;
        debug!(target: LOG_TARGET, "🌍️ Shard group {shard_group}: {reason}. Retrying in {interval:.0?}");
        tokio::select! {
            _ = cancel.cancelled() => {},
            Ok(()) = epoch.changed() => {},
            _ = time::sleep(interval) => {},
        }
    }

    /// Syncs every given shard from `session`, which serves them all over a single stream.
    ///
    /// A shard that has never been synced wants only the current head state rather than its full
    /// history, which is expressed by the `UP_ONLY` filter. Filters apply to the whole request, so
    /// such shards are streamed separately, on a stream that runs to its tip and closes. Every shard
    /// then joins the followed stream: one the head fetch found nothing for follows from version one,
    /// since its first transition is its head state, and it has to arrive while the stream is open
    /// rather than on the reopen after the deadline. The head fetch advances a shard only to the
    /// latest version it delivered, so the followed stream picks up the versions after it, which
    /// hold only substates since destroyed.
    async fn sync_shard_group_state(
        &mut self,
        shards: impl Iterator<Item = Shard>,
        progress: &SharedSyncProgress,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
        cancel: &CancellationToken,
        epoch: &mut watch::Receiver<Epoch>,
    ) -> Result<StreamEnd, NetworkStateSyncError> {
        let value_filters = SubstateValueFilterFlags::UTXO |
            SubstateValueFilterFlags::VALIDATOR_FEE_POOL |
            SubstateValueFilterFlags::CLAIMED_OUTPUT_TOMBSTONE |
            SubstateValueFilterFlags::TRANSACTION_RECEIPT |
            SubstateValueFilterFlags::TEMPLATE_METADATA;

        let shards = shards.collect::<Vec<_>>();
        let from_scratch = cursors_for(&shards, &*progress.lock().await)
            .into_iter()
            .filter(|cursor| cursor.start_state_version == 1)
            .collect::<Vec<_>>();
        if !from_scratch.is_empty() {
            info!(
                target: LOG_TARGET,
                "🌍️ Syncing {} shard(s) in shard group {shard_group} from scratch. Only fetching the head state.",
                from_scratch.len()
            );
            let end = self
                .stream_shard_state(
                    from_scratch,
                    value_filters | SubstateValueFilterFlags::UP_ONLY,
                    false,
                    progress,
                    shard_group,
                    session,
                    cancel,
                    epoch,
                )
                .await?;
            if matches!(end, StreamEnd::Cancelled | StreamEnd::EpochAdvanced) {
                return Ok(end);
            }
        }

        let cursors = cursors_for(&shards, &*progress.lock().await);
        // ALL_HASHES adds an id and a version for every substate outside the value filter, which is
        // what lets the substate cache tell a superseded or destroyed entry from a current one. It
        // is pointless on the from-scratch stream: those shards have no cached entries to retire.
        self.stream_shard_state(
            cursors,
            value_filters | SubstateValueFilterFlags::ALL_HASHES,
            true,
            progress,
            shard_group,
            session,
            cancel,
            epoch,
        )
        .await
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

    /// Consumes a single `sync_state` stream covering `cursors`, following the responder's tip if
    /// `follow` is set.
    ///
    /// The responder streams each shard's updates contiguously and closes it off with a completion
    /// marker, so progress is recorded per shard as the stream advances - an interrupted stream keeps
    /// everything already committed and simply resumes from the recorded cursors when reopened. A
    /// followed stream keeps going past the tip, closing off each burst of a shard's new versions
    /// with a further marker; it ends when the responder has had nothing to send for the deadline,
    /// or can no longer serve it.
    ///
    /// Cancellation and an epoch change are honoured between messages, so a version being committed
    /// when either arrives is committed in full and the recorded progress reflects it.
    #[expect(clippy::too_many_lines, clippy::too_many_arguments)]
    async fn stream_shard_state(
        &mut self,
        cursors: Vec<rpc::ShardCursor>,
        value_filters: SubstateValueFilterFlags,
        follow: bool,
        progress: &SharedSyncProgress,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
        cancel: &CancellationToken,
        epoch: &mut watch::Receiver<Epoch>,
    ) -> Result<StreamEnd, NetworkStateSyncError> {
        let mut order = StreamOrder::new(
            &cursors,
            *epoch.borrow(),
            value_filters.include_filtered_hashes() && !value_filters.is_up_only(),
        );

        info!(
            target: LOG_TARGET,
            "🌍️ Starting state sync for {} shard(s) in shard group {shard_group} from peer {} (follow: {follow})",
            cursors.len(),
            session.peer_address()
        );

        let options = RpcRequestOptions::new()
            .with_deadline(self.config.stream_deadline)
            .with_keepalive_interval(self.config.keepalive_interval);
        let mut stream = session
            .sync_state_with_options(
                rpc::SyncStateRequest {
                    cursors,
                    // Sync to latest epoch
                    until_epoch: None,
                    value_filters: value_filters.bits(),
                    follow,
                },
                options,
            )
            .await?;

        // A keepalive says the responder is there and has nothing to send: for every shard it has
        // closed off on this stream, that is the claim a further marker would make, so each one
        // re-stamps those shards' watermarks. A shard not yet closed off is still being caught up
        // and is not level, keepalive or not.
        let mut keepalives = stream.keepalives();
        let mut keepalives_open = true;
        let mut level_shards = HashSet::new();

        // Buffers accumulate a single (shard, state version) at a time: the responder splits an
        // oversized version into chunks flagged `has_more`, and the last chunk flushes them.
        let mut update_buf = Vec::new();
        let mut invalidations_buf = Vec::new();
        let mut utxos_buf = Vec::new();
        let mut transactions_buf = Vec::new();
        let mut validator_fee_pools_buf = Vec::new();
        let mut template_catalogue_buf: Vec<(TemplateAddress, PublishedTemplateMetadata)> = Vec::new();
        let mut xtr_claimed = Amount::zero();
        let mut xtr_fees = Amount::zero();
        let mut xtr_receipt_burn = Amount::zero();

        loop {
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(StreamEnd::Cancelled),
                Ok(()) = epoch.changed() => return Ok(StreamEnd::EpochAdvanced),
                next = stream.next() => match next {
                    Some(result) => result,
                    None => break,
                },
                changed = keepalives.changed(), if keepalives_open => {
                    match changed {
                        Ok(()) => {
                            for shard in &level_shards {
                                self.shard_watermarks.refresh(*shard);
                            }
                        },
                        Err(_) => keepalives_open = false,
                    }
                    continue;
                },
            };
            let msg = match result {
                Ok(msg) => msg,
                // A followed stream with nothing to send for the deadline is simply abandoned by the
                // responder, which the client reports as a timeout.
                Err(status) if follow && status.as_status_code().is_timeout() => {
                    debug!(target: LOG_TARGET, "🌍️ State sync stream for shard group {shard_group} timed out: {status}");
                    return Ok(StreamEnd::TimedOut);
                },
                Err(status) => return Err(status.into()),
            };
            let batch = match msg.response {
                Some(rpc::sync_state_response::Response::Batch(batch)) => batch,
                Some(rpc::sync_state_response::Response::Complete(complete)) => {
                    let shard = Shard::from(complete.shard);
                    let synced_to = StateVersion::new(complete.synced_to_version);
                    let msg_epoch =
                        complete
                            .epoch
                            .map(Epoch::from)
                            .ok_or_else(|| NetworkStateSyncError::InvalidStateUpdate {
                                details: "Received sync completion without epoch".to_string(),
                            })?;
                    // Recorded progress only ever advances with a committed batch, never to the
                    // version a marker claims: a claim past what was streamed would skip every
                    // version in between for good.
                    let level_at = order
                        .accept_marker(shard, synced_to, msg_epoch)
                        .map_err(|details| NetworkStateSyncError::InvalidStateUpdate { details })?;
                    // The completion marker is the only point at which this shard is known to be
                    // level with the committee, which is what the substate cache needs: mid-stream
                    // the indexer holds every transition up to some version while the chain is
                    // arbitrarily far ahead of it. Confirmed even when the watermark did not move -
                    // a quiet shard is still one this indexer is keeping up with.
                    if let Some(level_at) = level_at {
                        self.shard_watermarks.confirm(shard, level_at);
                        level_shards.insert(shard);
                    }
                    debug!(target: LOG_TARGET, "🌍️ Completed state sync for shard {shard} in shard group {shard_group} at epoch {msg_epoch} (peer synced to v{synced_to})");
                    if complete.is_final {
                        return Ok(StreamEnd::Final);
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
            let msg_epoch = batch
                .epoch
                .map(Epoch::from)
                .ok_or_else(|| NetworkStateSyncError::InvalidStateUpdate {
                    details: "Received state update without epoch".to_string(),
                })?;
            order
                .accept_batch(shard, state_version, msg_epoch, batch.has_more, batch.encoded_len())
                .map_err(|details| NetworkStateSyncError::InvalidStateUpdate { details })?;

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
                    value_filters,
                    &mut update_buf,
                    &mut invalidations_buf,
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
            let invalidations = std::mem::take(&mut invalidations_buf);
            let utxos = std::mem::take(&mut utxos_buf);
            let transactions = std::mem::take(&mut transactions_buf);
            let validator_fee_pools = std::mem::take(&mut validator_fee_pools_buf);
            let template_catalogue = std::mem::take(&mut template_catalogue_buf);

            let updates_len = updates.len();
            let utxos_len = utxos.len();
            let transactions_len = transactions.len();
            let template_catalogue_len = template_catalogue.len();
            let event_count: usize = transactions.iter().map(|(_, t)| t.events.len()).sum();
            self.stats.increase_events(event_count);

            let mut progress = progress.lock().await;
            progress.record_state_version(shard, state_version, msg_epoch);
            let sync_progress_snapshot = progress.clone();

            let network = self.network;
            let event_filters = self.config.event_filters.clone();
            let watched_templates = self.config.watched_templates.clone();
            let xtr_claimed_snapshot = xtr_claimed;
            let xtr_fees_snapshot = xtr_fees;
            let xtr_receipt_burn_snapshot = xtr_receipt_burn;

            let (inserted_events, retired_cache_entries) = self
                .store
                .clone()
                .with_write_tx(move |tx| -> Result<(Vec<InsertedEvent>, usize), StorageError> {
                    debug!(target: LOG_TARGET, "✅ Committing {} updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})", updates_len);
                    // TODO: this is not currently used. Consider removing.
                    tx.batch_insert_substate_transitions(network, shard, state_version, updates)?;
                    // Must commit with the watermark below: the substate cache serves an entry on the
                    // argument that it holds every transition up to that watermark, which a reader
                    // seeing one of the two without the other would break.
                    let retired_cache_entries = tx.substate_cache_invalidate(invalidations, state_version)?;
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
                    Ok((inserted, retired_cache_entries))
                })
                .await?;
            drop(progress);
            if retired_cache_entries > 0 {
                debug!(target: LOG_TARGET, "Retired {retired_cache_entries} cached substates for shard {shard} at state version {state_version}");
                #[cfg(feature = "metrics")]
                self.substate_cache_metrics.add_invalidations(retired_cache_entries);
            }

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

        // A followed stream is only ever ended by the responder for want of its warrant, which it
        // reports, or of consensus. Ending silently is the peer's failing either way.
        Err(NetworkStateSyncError::InvalidStateUpdate {
            details: if follow {
                format!("Followed state sync stream for shard group {shard_group} was closed by the responder")
            } else {
                format!("State sync stream for shard group {shard_group} ended without a final completion marker")
            },
        })
    }
}

/// Whether a plan drawn at `plan_epoch` against `partition` carries on into the epoch `network_desc`
/// describes, with each shard group reopening on its own.
///
/// It does so only while the partition of shards into groups is unchanged: a group loop serves one
/// set of shards for its lifetime. A plan drawn at epoch zero has no group loops at all - it waits
/// for the network to start - so the first real epoch always draws a new plan, whatever partition
/// epoch zero reported.
fn plan_absorbs_epoch(plan_epoch: Epoch, partition: &BTreeSet<ShardGroup>, network_desc: &NetworkDescription) -> bool {
    !plan_epoch.is_zero() && network_desc.shard_groups_iter().collect::<BTreeSet<_>>() == *partition
}

/// How a `sync_state` stream ended without failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamEnd {
    /// The responder closed it with a final completion marker: it streamed to its tip and does not
    /// follow.
    Final,
    /// A followed stream was abandoned by the responder after it had nothing to send for the
    /// deadline.
    TimedOut,
    /// The epoch moved while the stream was open. The group syncs the new checkpoints and reopens
    /// from a committee member at the new epoch.
    EpochAdvanced,
    /// The plan is being wound down.
    Cancelled,
}

/// Rewinds every shard whose recorded progress no stream could have legitimately delivered - a
/// version above [`MAX_STATE_VERSION`] or an epoch past `current_epoch` - returning the shards
/// rewound.
fn rewind_out_of_range_progress<TTx: IndexerStoreReadTransaction>(
    tx: &mut TTx,
    progress: &mut SyncProgress,
    current_epoch: Epoch,
) -> Result<Vec<Shard>, StorageError> {
    let out_of_range = progress.shards_recorded_beyond(MAX_STATE_VERSION, current_epoch);
    for shard in &out_of_range {
        rewind_shard(tx, progress, *shard, MAX_STATE_VERSION)?;
    }
    Ok(out_of_range)
}

/// Rewinds every shard whose recorded progress, made at or before `checkpoint_epoch`, is ahead of
/// the version a validated checkpoint at that epoch commits the shard to, returning the shards
/// rewound. The checkpoint is quorum-signed and a shard's version only grows, so such progress came
/// from a peer claiming versions its committee never committed.
fn reconcile_progress_with_checkpoint<TTx: IndexerStoreReadTransaction>(
    tx: &mut TTx,
    progress: &mut SyncProgress,
    checkpoint_epoch: Epoch,
    checkpoint_versions: impl IntoIterator<Item = (Shard, StateVersion)>,
) -> Result<Vec<Shard>, StorageError> {
    let mut rewound = Vec::new();
    for (shard, checkpoint_version) in checkpoint_versions {
        let Some(&(recorded_version, recorded_epoch)) = progress.last_state_versions.get(&shard) else {
            continue;
        };
        if recorded_epoch <= checkpoint_epoch && recorded_version > checkpoint_version {
            rewind_shard(tx, progress, shard, checkpoint_version)?;
            rewound.push(shard);
        }
    }
    Ok(rewound)
}

fn log_rewound(progress: &SyncProgress, shards: &[Shard], reason: &str) {
    for shard in shards {
        match progress.last_state_version(*shard) {
            Some(version) => {
                error!(target: LOG_TARGET, "⚠️ {reason} for shard {shard}. Resuming it after v{version}");
            },
            None => {
                error!(target: LOG_TARGET, "⚠️ {reason} for shard {shard}. Resyncing it from its head state");
            },
        }
    }
}

/// Rewinds `shard`'s recorded progress, which its committee never reached, to the highest version
/// at which a transition was committed for it, or leaves it unrecorded to resume from scratch if
/// there is none.
///
/// That is the lowest point the shard resumes from without re-applying anything: every row and
/// running total a version contributes is committed with a transition at that version, so the
/// versions above it that are re-streamed carried nothing but cache invalidations, which are
/// idempotent. A transition committed above `committed_to`, the highest version the committee is
/// known to have reached, was forged by a peer; the shard resumes after it all the same, since
/// re-streaming beneath a committed row would collide with it.
fn rewind_shard<TTx: IndexerStoreReadTransaction>(
    tx: &mut TTx,
    progress: &mut SyncProgress,
    shard: Shard,
    committed_to: StateVersion,
) -> Result<(), StorageError> {
    progress.forget_state_version(shard);
    if tx.substate_transitions_exist_above(shard, committed_to)? {
        error!(
            target: LOG_TARGET,
            "⚠️ Shard {shard} holds state transitions above v{committed_to}, which its committee never committed. The \
             indexed data and economic totals for this shard cannot be trusted; resync this indexer from an empty data \
             directory"
        );
    }
    if let Some((state_version, epoch)) = tx.substate_transitions_get_latest_state_version(shard, MAX_STATE_VERSION)? {
        progress.record_state_version(shard, state_version, epoch);
    }
    Ok(())
}

/// A cursor per shard resuming after the version recorded for it, in the order of `shards`. A shard
/// never synced resumes from version one.
fn cursors_for(shards: &[Shard], progress: &SyncProgress) -> Vec<rpc::ShardCursor> {
    shards
        .iter()
        .map(|&shard| rpc::ShardCursor {
            shard: shard.as_u32(),
            start_state_version: progress
                .last_state_version(shard)
                .map_or(1, |v| v.as_u64().saturating_add(1)),
        })
        .collect()
}

/// Enforces the ordering a `sync_state` stream promises across the shards it carries: a shard's
/// versions strictly advance, and a version split across chunks is delivered whole before anything
/// else.
///
/// The consumer relies on both. Its buffers hold one `(shard, state version)` at a time, so a shard
/// interleaved mid-version would mix two shards' updates into one commit; and because the running
/// economic totals are read-modify-write, re-applying a version already committed would double-count
/// it. A completion marker closes off what was streamed so far for a shard and a followed stream
/// carries more for it after, so a marker does not end a shard.
///
/// Every message is also held to at most the epoch after the stream's: progress is recorded at the
/// epoch a message names, and reconciled against the checkpoint for that epoch once it closes,
/// which a message naming an epoch further ahead would put out of reach. And a version split across
/// chunks is held to [`MAX_BUFFERED_VERSION_BYTES`], since it is buffered whole until the chunk
/// that flushes it.
struct StreamOrder {
    /// Highest version committed per requested shard, seeded from the cursor so a responder cannot
    /// replay versions the caller already holds.
    committed_versions: HashMap<Shard, StateVersion>,
    /// Set while a version is split across chunks, with the encoded bytes buffered for it, until the
    /// chunk that flushes it.
    pending_chunk: Option<(Shard, StateVersion, usize)>,
    /// The epoch this indexer is at, which the stream ends on leaving.
    epoch: Epoch,
    /// Whether the stream carries every transition of its shards. Every committed state version of
    /// a shard holds at least one transition, so such a stream delivers each shard's versions
    /// contiguously from its cursor, and a marker claims no version beyond the last one delivered.
    complete: bool,
}

impl StreamOrder {
    fn new(cursors: &[rpc::ShardCursor], epoch: Epoch, complete: bool) -> Self {
        Self {
            epoch,
            complete,
            committed_versions: cursors
                .iter()
                .map(|c| {
                    (
                        Shard::from(c.shard),
                        StateVersion::new(c.start_state_version.saturating_sub(1)),
                    )
                })
                .collect(),
            pending_chunk: None,
        }
    }

    fn accept_batch(
        &mut self,
        shard: Shard,
        state_version: StateVersion,
        epoch: Epoch,
        has_more: bool,
        encoded_bytes: usize,
    ) -> Result<(), String> {
        let Some(committed_version) = self.committed_versions.get(&shard).copied() else {
            return Err(format!("Received batch for unrequested shard {shard}"));
        };
        self.check_bounds(shard, state_version, epoch)?;
        if state_version <= committed_version {
            return Err(format!(
                "Received v{state_version} for shard {shard}, which is not ahead of the committed v{committed_version}"
            ));
        }
        let buffered_bytes = match self.pending_chunk {
            Some((pending_shard, pending_version, bytes))
                if (pending_shard, pending_version) == (shard, state_version) =>
            {
                bytes
            },
            Some((pending_shard, pending_version, _)) => {
                return Err(format!(
                    "Received v{state_version} of shard {shard} while v{pending_version} of shard {pending_shard} is \
                     still incomplete"
                ));
            },
            None => {
                if self.complete && state_version.as_u64() != committed_version.as_u64() + 1 {
                    return Err(format!(
                        "Received v{state_version} for shard {shard}, skipping the versions after the committed \
                         v{committed_version}"
                    ));
                }
                0
            },
        }
        .saturating_add(encoded_bytes);
        if buffered_bytes > MAX_BUFFERED_VERSION_BYTES {
            return Err(format!(
                "Received more than {MAX_BUFFERED_VERSION_BYTES} bytes for v{state_version} of shard {shard} without \
                 its completion"
            ));
        }

        if has_more {
            self.pending_chunk = Some((shard, state_version, buffered_bytes));
        } else {
            self.pending_chunk = None;
            self.committed_versions.insert(shard, state_version);
        }
        Ok(())
    }

    /// Accepts a completion marker, returning the version the shard is now level with the committee
    /// at: the last version delivered for it. A stream that leaves out transitions establishes no
    /// such version.
    fn accept_marker(
        &mut self,
        shard: Shard,
        synced_to: StateVersion,
        epoch: Epoch,
    ) -> Result<Option<StateVersion>, String> {
        let Some(committed_version) = self.committed_versions.get(&shard).copied() else {
            return Err(format!("Received completion marker for unrequested shard {shard}"));
        };
        self.check_bounds(shard, synced_to, epoch)?;
        if let Some((pending_shard, pending_version, _)) = self.pending_chunk {
            return Err(format!(
                "Received completion marker for shard {shard} while v{pending_version} of shard {pending_shard} is \
                 still incomplete"
            ));
        }
        if !self.complete {
            return Ok(None);
        }
        if synced_to > committed_version {
            return Err(format!(
                "Received completion marker for shard {shard} at v{synced_to}, past the last version streamed, \
                 v{committed_version}"
            ));
        }
        Ok(Some(committed_version))
    }

    /// An honest peer's consensus may enter an epoch before this indexer's base layer scan does, so a
    /// message may name the epoch after the indexer's. That still leaves what it records within reach
    /// of the checkpoint that closes the epoch it names.
    fn check_bounds(&self, shard: Shard, state_version: StateVersion, epoch: Epoch) -> Result<(), String> {
        if state_version > MAX_STATE_VERSION {
            return Err(format!(
                "Received v{state_version} for shard {shard}, which exceeds the maximum v{MAX_STATE_VERSION}"
            ));
        }
        let max_epoch = next_epoch(self.epoch);
        if epoch > max_epoch {
            return Err(format!(
                "Received v{state_version} for shard {shard} at {epoch}, which is ahead of {max_epoch}"
            ));
        }
        Ok(())
    }
}

fn next_epoch(epoch: Epoch) -> Epoch {
    Epoch(epoch.as_u64().saturating_add(1))
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
                        .get_as::<TemplateAddress>("prev_template")
                        .ok()
                        .flatten();

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

/// Sorts one streamed transition into the buffers a commit is assembled from.
///
/// A transition that retires anything cached reaches `invalidations_buf`, which for a substate's
/// first creation is the record that it did not exist. Only those whose substate `value_filters` selects carry a
/// value; the rest arrive as an id and a version under `ALL_HASHES` and must reach nothing else -
/// indexing one, counting it in the economic totals or emitting an event for it would all be reading
/// a value that was never sent.
fn extend_bufs_from_substate_update(
    notify: &Notify<IndexerEvent>,
    shard: Shard,
    state_version: StateVersion,
    update: SubstateUpdateProof,
    msg_epoch: Epoch,
    value_filters: SubstateValueFilterFlags,
    update_buf: &mut Vec<(Epoch, SubstateUpdateProof)>,
    invalidations_buf: &mut Vec<SubstateCacheInvalidation>,
    utxos_buf: &mut Vec<UtxoUpdateRecord>,
    transactions_buf: &mut Vec<(TransactionReceiptAddress, TransactionReceipt)>,
    validator_fee_pools_buf: &mut Vec<SubstateData>,
    template_catalogue_buf: &mut Vec<(TemplateAddress, PublishedTemplateMetadata)>,
    xtr_claimed_mut: &mut Amount,
    xtr_fees_mut: &mut Amount,
    xtr_receipt_burn_mut: &mut Amount,
) -> Result<(), NetworkStateSyncError> {
    invalidations_buf.extend(match &update {
        SubstateUpdateProof::Create(create) => {
            SubstateCacheInvalidation::created(create.substate.substate_id(), create.substate.version)
        },
        SubstateUpdateProof::Destroy(destroy) => Some(SubstateCacheInvalidation::destroyed(
            destroy.substate_id.clone(),
            destroy.version,
        )),
    });

    if !value_filters.contains_substate(update.substate_id()) {
        return Ok(());
    }

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
                        // Accumulate the realized-share pair from the same receipt: what the payer spent
                        // and the burn taken out of it, so `burn / paid` recovers the share independent
                        // of the header-sourced burn total.
                        let fee_receipt = receipt.fee_receipt();
                        *xtr_receipt_burn_mut += Amount::from(fee_receipt.exhaust_burn());
                        *xtr_fees_mut += Amount::from(fee_receipt.total_fees_paid());

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

    mod plan_absorbs_epoch {
        use tari_epoch_manager::service::ShardGroupInfo;
        use tari_ootle_common_types::NumPreshards;

        use super::*;

        fn network(epoch: u64, groups: &[ShardGroup]) -> NetworkDescription {
            NetworkDescription {
                epoch: Epoch(epoch),
                shard_groups: groups.iter().map(|g| (*g, ShardGroupInfo { num_members: 1 })).collect(),
                num_preshards: NumPreshards::P256,
            }
        }

        fn partition(groups: &[ShardGroup]) -> BTreeSet<ShardGroup> {
            groups.iter().copied().collect()
        }

        fn whole() -> ShardGroup {
            ShardGroup::new(1u32, 256)
        }

        #[test]
        fn an_epoch_that_keeps_the_partition_is_absorbed() {
            assert!(plan_absorbs_epoch(
                Epoch(3),
                &partition(&[whole()]),
                &network(4, &[whole()])
            ));
        }

        #[test]
        fn a_changed_partition_draws_a_new_plan() {
            let split = [ShardGroup::new(1u32, 128), ShardGroup::new(129u32, 256)];
            assert!(!plan_absorbs_epoch(
                Epoch(3),
                &partition(&[whole()]),
                &network(4, &split)
            ));
        }

        #[test]
        fn a_plan_drawn_at_epoch_zero_never_absorbs_the_first_epoch() {
            // Epoch zero reports a partition, but the plan drawn against it runs no group loops.
            assert!(!plan_absorbs_epoch(
                Epoch(0),
                &partition(&[whole()]),
                &network(1, &[whole()])
            ));
        }
    }

    mod stream_order {
        use super::*;

        const S1: Shard = Shard::from_u32(1);
        const S2: Shard = Shard::from_u32(2);
        const E: Epoch = Epoch(5);

        fn cursors(cursors: &[(u32, u64)]) -> Vec<rpc::ShardCursor> {
            cursors
                .iter()
                .map(|&(shard, start_state_version)| rpc::ShardCursor {
                    shard,
                    start_state_version,
                })
                .collect()
        }

        /// A followed stream, which carries every transition.
        fn order(c: &[(u32, u64)]) -> StreamOrder {
            StreamOrder::new(&cursors(c), E, true)
        }

        /// A head fetch, which leaves out transitions of substates since destroyed.
        fn head_order(c: &[(u32, u64)]) -> StreamOrder {
            StreamOrder::new(&cursors(c), E, false)
        }

        fn v(n: u64) -> StateVersion {
            StateVersion::new(n)
        }

        #[test]
        fn it_accepts_shards_streamed_one_after_another() {
            let mut o = order(&[(1, 1), (2, 5)]);
            o.accept_batch(S1, v(1), E, false, 0).unwrap();
            o.accept_batch(S1, v(2), E, false, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(2), E).unwrap(), Some(v(2)));
            o.accept_batch(S2, v(5), E, false, 0).unwrap();
            assert_eq!(o.accept_marker(S2, v(5), E).unwrap(), Some(v(5)));
        }

        #[test]
        fn it_accepts_a_version_split_across_chunks() {
            let mut o = order(&[(1, 3)]);
            o.accept_batch(S1, v(3), E, true, 0).unwrap();
            o.accept_batch(S1, v(3), E, true, 0).unwrap();
            o.accept_batch(S1, v(3), E, false, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(3), E).unwrap(), Some(v(3)));
        }

        #[test]
        fn it_rejects_a_version_below_the_cursor() {
            // A cursor of 5 asks to resume at v5, so v5 is wanted and everything under it is already held.
            assert!(order(&[(1, 5)]).accept_batch(S1, v(3), E, false, 0).is_err());
            assert!(order(&[(1, 5)]).accept_batch(S1, v(4), E, false, 0).is_err());
            order(&[(1, 5)]).accept_batch(S1, v(5), E, false, 0).unwrap();
        }

        #[test]
        fn it_rejects_a_replayed_version() {
            let mut o = order(&[(1, 9)]);
            o.accept_batch(S1, v(9), E, false, 0).unwrap();
            assert!(o.accept_batch(S1, v(9), E, false, 0).is_err());
        }

        #[test]
        fn it_rejects_a_regressing_version() {
            let mut o = head_order(&[(1, 1)]);
            o.accept_batch(S1, v(9), E, false, 0).unwrap();
            assert!(o.accept_batch(S1, v(8), E, false, 0).is_err());
        }

        #[test]
        fn a_followed_stream_rejects_a_skipped_version() {
            assert!(order(&[(1, 1)]).accept_batch(S1, v(2), E, false, 0).is_err());
            assert!(order(&[(1, 1)]).accept_batch(S1, v(2), E, true, 0).is_err());
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(1), E, false, 0).unwrap();
            assert!(o.accept_batch(S1, v(3), E, false, 0).is_err());
        }

        #[test]
        fn a_followed_stream_rejects_a_marker_past_the_last_version_streamed() {
            let mut o = order(&[(1, 101)]);
            assert!(o.accept_marker(S1, v(100_000), E).is_err());
            assert!(o.accept_marker(S1, v(101), E).is_err());
            o.accept_batch(S1, v(101), E, false, 0).unwrap();
            assert!(o.accept_marker(S1, v(102), E).is_err());
            assert_eq!(o.accept_marker(S1, v(101), E).unwrap(), Some(v(101)));
        }

        #[test]
        fn a_head_fetch_skips_versions_and_establishes_no_level() {
            let mut o = head_order(&[(1, 1)]);
            o.accept_batch(S1, v(4), E, false, 0).unwrap();
            o.accept_batch(S1, v(9), E, false, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(100_000), E).unwrap(), None);
        }

        #[test]
        fn it_accepts_a_followed_shard_streamed_again_after_its_marker() {
            let mut o = order(&[(1, 2), (2, 1)]);
            o.accept_batch(S1, v(2), E, false, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(2), E).unwrap(), Some(v(2)));
            assert_eq!(o.accept_marker(S2, v(0), E).unwrap(), Some(v(0)));
            o.accept_batch(S1, v(3), E, false, 0).unwrap();
            o.accept_marker(S1, v(3), E).unwrap();
            // A forced marker on an epoch change closes off nothing new.
            assert_eq!(o.accept_marker(S1, v(3), E).unwrap(), Some(v(3)));
            assert!(o.accept_batch(S1, v(3), E, false, 0).is_err());
        }

        #[test]
        fn it_rejects_an_interleaved_shard_mid_version() {
            let mut o = order(&[(1, 3), (2, 1)]);
            o.accept_batch(S1, v(3), E, true, 0).unwrap();
            assert!(o.accept_batch(S2, v(1), E, false, 0).is_err());
        }

        #[test]
        fn it_rejects_a_marker_while_a_version_is_incomplete() {
            let mut o = order(&[(1, 3)]);
            o.accept_batch(S1, v(3), E, true, 0).unwrap();
            assert!(o.accept_marker(S1, v(3), E).is_err());
        }

        #[test]
        fn it_rejects_unrequested_shards() {
            assert!(order(&[(1, 1)]).accept_batch(S2, v(1), E, false, 0).is_err());
            assert!(order(&[(1, 1)]).accept_marker(S2, v(1), E).is_err());
        }

        #[test]
        fn it_rejects_a_marker_above_the_maximum_version() {
            let mut o = head_order(&[(1, 1)]);
            o.accept_marker(S1, MAX_STATE_VERSION, E).unwrap();
            assert!(o.accept_marker(S1, v(MAX_STATE_VERSION.as_u64() + 1), E).is_err());
            assert!(o.accept_marker(S1, v(u64::MAX), E).is_err());
        }

        #[test]
        fn it_rejects_a_batch_above_the_maximum_version() {
            assert!(
                head_order(&[(1, 1)])
                    .accept_batch(S1, v(u64::MAX), E, false, 0)
                    .is_err()
            );
            assert!(head_order(&[(1, 1)]).accept_batch(S1, v(u64::MAX), E, true, 0).is_err());
            head_order(&[(1, 1)])
                .accept_batch(S1, MAX_STATE_VERSION, E, false, 0)
                .unwrap();
        }

        #[test]
        fn it_accepts_the_epoch_after_the_current_one_and_no_further() {
            let mut o = order(&[(1, 1)]);
            o.accept_marker(S1, v(0), Epoch(4)).unwrap();
            o.accept_marker(S1, v(0), E).unwrap();
            o.accept_marker(S1, v(0), Epoch(6)).unwrap();
            assert!(o.accept_marker(S1, v(0), Epoch(7)).is_err());
            assert!(o.accept_marker(S1, v(0), Epoch(u64::MAX)).is_err());
            assert!(order(&[(1, 1)]).accept_batch(S1, v(1), Epoch(7), false, 0).is_err());
            assert!(order(&[(1, 1)]).accept_batch(S1, v(1), Epoch(7), true, 0).is_err());
            order(&[(1, 1)]).accept_batch(S1, v(1), Epoch(6), false, 0).unwrap();
        }

        #[test]
        fn it_rejects_a_version_streamed_past_the_byte_budget() {
            const CHUNK: usize = 6 * 1024 * 1024;
            let mut o = order(&[(1, 3)]);
            for _ in 0..MAX_BUFFERED_VERSION_BYTES / CHUNK {
                o.accept_batch(S1, v(3), E, true, CHUNK).unwrap();
            }
            assert!(o.accept_batch(S1, v(3), E, true, CHUNK).is_err());
        }

        #[test]
        fn the_byte_budget_applies_to_each_version_afresh() {
            let mut o = order(&[(1, 3)]);
            o.accept_batch(S1, v(3), E, true, MAX_BUFFERED_VERSION_BYTES - 1)
                .unwrap();
            o.accept_batch(S1, v(3), E, false, 1).unwrap();
            o.accept_batch(S1, v(4), E, true, MAX_BUFFERED_VERSION_BYTES - 1)
                .unwrap();
            assert!(o.accept_batch(S1, v(4), E, false, 2).is_err());
        }
    }

    mod cursors_for {
        use super::*;

        const S1: Shard = Shard::from_u32(1);

        fn progress_at(version: u64) -> SyncProgress {
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, StateVersion::new(version), Epoch(1));
            progress
        }

        #[test]
        fn it_resumes_after_the_recorded_version() {
            assert_eq!(cursors_for(&[S1], &progress_at(12_345))[0].start_state_version, 12_346);
            assert_eq!(cursors_for(&[S1], &SyncProgress::default())[0].start_state_version, 1);
        }

        #[test]
        fn it_does_not_overflow_on_the_largest_version() {
            let cursors = cursors_for(&[S1], &progress_at(u64::MAX));
            assert_eq!(cursors[0].start_state_version, u64::MAX);
        }
    }

    mod rewind_out_of_range_progress {
        use tari_engine_types::substate::SubstateId;
        use tari_ootle_common_types::SubstateVersion;
        use tari_ootle_storage::consensus_models::SubstateDestroy;
        use tari_template_lib_types::ValidatorFeePoolAddress;

        use super::*;

        const S1: Shard = Shard::from_u32(1);
        const S2: Shard = Shard::from_u32(2);

        fn destroyed(n: u8) -> SubstateUpdateProof {
            SubstateUpdateProof::Destroy(SubstateDestroy {
                substate_id: SubstateId::ValidatorFeePool(ValidatorFeePoolAddress::from_array([n; 32])),
                version: SubstateVersion::new(0),
            })
        }

        async fn store_with_transitions(transitions: &[(Shard, u64, u64)]) -> (tempfile::TempDir, SqliteIndexerStore) {
            let dir = tempfile::tempdir().unwrap();
            let store = SqliteIndexerStore::try_create(dir.path().join("indexer.db")).unwrap();
            let transitions = transitions.to_vec();
            store
                .with_write_tx(move |tx| {
                    for (n, (shard, state_version, epoch)) in transitions.into_iter().enumerate() {
                        tx.batch_insert_substate_transitions(
                            Network::LocalNet,
                            shard,
                            StateVersion::new(state_version),
                            [(Epoch(epoch), destroyed(n as u8))],
                        )?;
                    }
                    Ok::<_, StorageError>(())
                })
                .await
                .unwrap();
            (dir, store)
        }

        const CURRENT_EPOCH: Epoch = Epoch(5);

        async fn rewind(store: &SqliteIndexerStore, progress: SyncProgress) -> (SyncProgress, Vec<Shard>) {
            store
                .with_read_tx(move |tx| {
                    let mut progress = progress;
                    let rewound = rewind_out_of_range_progress(tx, &mut progress, CURRENT_EPOCH)?;
                    Ok::<_, StorageError>((progress, rewound))
                })
                .await
                .unwrap()
        }

        async fn reconcile(
            store: &SqliteIndexerStore,
            progress: SyncProgress,
            checkpoint_epoch: Epoch,
            checkpoint_versions: &[(Shard, u64)],
        ) -> (SyncProgress, Vec<Shard>) {
            let checkpoint_versions = checkpoint_versions
                .iter()
                .map(|&(shard, version)| (shard, StateVersion::new(version)))
                .collect::<Vec<_>>();
            store
                .with_read_tx(move |tx| {
                    let mut progress = progress;
                    let rewound =
                        reconcile_progress_with_checkpoint(tx, &mut progress, checkpoint_epoch, checkpoint_versions)?;
                    Ok::<_, StorageError>((progress, rewound))
                })
                .await
                .unwrap()
        }

        #[tokio::test]
        async fn it_resumes_an_out_of_range_shard_after_its_last_committed_transition() {
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2), (S1, 9, 3), (S2, 100, 4)]).await;
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, StateVersion::new(u64::MAX), Epoch(5));
            progress.record_state_version(S2, StateVersion::new(120), Epoch(5));

            let (progress, rewound) = rewind(&store, progress).await;

            assert_eq!(rewound, vec![S1]);
            assert_eq!(
                progress.last_state_versions.get(&S1),
                Some(&(StateVersion::new(9), Epoch(3)))
            );
            assert_eq!(progress.last_state_version(S2), Some(StateVersion::new(120)));
            assert_eq!(cursors_for(&[S1, S2], &progress)[0].start_state_version, 10);
        }

        #[tokio::test]
        async fn an_out_of_range_shard_with_no_transitions_resumes_from_scratch() {
            let (_dir, store) = store_with_transitions(&[(S2, 100, 4)]).await;
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, StateVersion::new(MAX_STATE_VERSION.as_u64() + 1), Epoch(5));

            let (progress, rewound) = rewind(&store, progress).await;

            assert_eq!(rewound, vec![S1]);
            assert_eq!(progress.last_state_version(S1), None);
            assert_eq!(cursors_for(&[S1], &progress)[0].start_state_version, 1);
        }

        #[tokio::test]
        async fn in_range_progress_is_left_alone() {
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2)]).await;
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, MAX_STATE_VERSION, Epoch(5));

            let (progress, rewound) = rewind(&store, progress).await;

            assert!(rewound.is_empty());
            assert_eq!(progress.last_state_version(S1), Some(MAX_STATE_VERSION));
        }

        #[tokio::test]
        async fn progress_recorded_at_a_future_epoch_is_rewound() {
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2)]).await;
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, StateVersion::new(50), Epoch(u64::MAX));

            let (progress, rewound) = rewind(&store, progress).await;

            assert_eq!(rewound, vec![S1]);
            assert_eq!(
                progress.last_state_versions.get(&S1),
                Some(&(StateVersion::new(5), Epoch(2)))
            );
        }

        #[tokio::test]
        async fn a_wrapped_transition_is_reported_and_skipped() {
            // A version above i64::MAX wraps negative in the signed column.
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2), (S1, u64::MAX, 3)]).await;
            let (exists_above, latest) = store
                .with_read_tx(|tx| {
                    Ok::<_, StorageError>((
                        tx.substate_transitions_exist_above(S1, MAX_STATE_VERSION)?,
                        tx.substate_transitions_get_latest_state_version(S1, MAX_STATE_VERSION)?,
                    ))
                })
                .await
                .unwrap();
            assert!(exists_above);
            assert_eq!(latest, Some((StateVersion::new(5), Epoch(2))));
        }

        #[tokio::test]
        async fn progress_ahead_of_the_checkpoint_resumes_after_the_last_committed_transition() {
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2), (S1, 9, 3)]).await;
            let mut progress = SyncProgress::default();
            // Claimed by a peer past anything its committee committed in epoch 3.
            progress.record_state_version(S1, StateVersion::new(100_000), Epoch(3));

            let (progress, rewound) = reconcile(&store, progress, Epoch(3), &[(S1, 12)]).await;

            assert_eq!(rewound, vec![S1]);
            assert_eq!(
                progress.last_state_versions.get(&S1),
                Some(&(StateVersion::new(9), Epoch(3)))
            );
            // Versions 10 to 12, skipped by the claim, are requested again.
            assert_eq!(cursors_for(&[S1], &progress)[0].start_state_version, 10);
        }

        #[tokio::test]
        async fn progress_the_checkpoint_does_not_contradict_is_left_alone() {
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2)]).await;
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, StateVersion::new(12), Epoch(3));
            // Recorded after the checkpoint's epoch, so ahead of it legitimately.
            progress.record_state_version(S2, StateVersion::new(50), Epoch(4));

            let (progress, rewound) = reconcile(&store, progress, Epoch(3), &[(S1, 12), (S2, 20)]).await;

            assert!(rewound.is_empty());
            assert_eq!(progress.last_state_version(S1), Some(StateVersion::new(12)));
            assert_eq!(progress.last_state_version(S2), Some(StateVersion::new(50)));
        }

        #[tokio::test]
        async fn a_shard_rewound_beneath_a_committed_transition_resumes_after_it() {
            // A transition above the checkpoint version was forged, but re-streaming beneath it would
            // collide with it.
            let (_dir, store) = store_with_transitions(&[(S1, 5, 2), (S1, 15, 3)]).await;
            let mut progress = SyncProgress::default();
            progress.record_state_version(S1, StateVersion::new(100_000), Epoch(3));

            let (progress, rewound) = reconcile(&store, progress, Epoch(3), &[(S1, 12)]).await;

            assert_eq!(rewound, vec![S1]);
            assert_eq!(progress.last_state_version(S1), Some(StateVersion::new(15)));
        }
    }
}
