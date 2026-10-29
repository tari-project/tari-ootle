//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    convert::Infallible,
    fmt::Display,
    pin::pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use futures::{StreamExt, future::Either, stream::FuturesUnordered};
use log::*;
use ootle_network::Network;
use prost::Message;
use tari_engine_types::{
    SubstateVersion,
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
use tari_ootle_common_types::{Epoch, ShardGroup, StateVersion, ValidatorSetHash, optional::Optional, shard::Shard};
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
use tari_rpc_framework::RpcRequestOptions;
use tari_shutdown::ShutdownSignal;
use tari_state_tree::SubstateTreeChange;
use tari_template_lib_types::{Amount, TemplateAddress, TransactionReceiptAddress, UtxoAddress};
use tokio::{
    sync::{self as tokio_sync, broadcast, mpsc, watch},
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
        EventFilter,
        committee_client::{ValidatorCommitteeRpcPool, ValidatorRpcSession},
        config::NetworkWideStateSyncConfig,
        consensus_epoch::ConsensusEpoch,
        error::NetworkStateSyncError,
        shard_watermarks::ShardWatermarks,
        stats::SyncStats,
        sync_plan::SyncPlan,
        sync_progress::{SharedSyncProgress, SyncProgress, VerifiedShardVersion},
        validator_status::ValidatorStatusMonitor,
        verified_stage::{
            MAX_ACCUMULATOR_HISTORY,
            StageError,
            Staged,
            StreamedTransitionError,
            Verdict,
            VerifiedShardLeaf,
            VerifiedStage,
            streamed_tree_change,
        },
    },
    notify::Notify,
    storage_sqlite::{
        SqliteIndexerStore,
        SqliteStoreWriteTransaction,
        models::{
            CheckpointShardLeaf,
            Key,
            StagedAccumulator,
            StagedVersion,
            SubstateCacheInvalidation,
            UtxoSpent,
            UtxoUnspent,
            UtxoUpdateRecord,
            VerifiedStateRoot,
        },
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
/// How often every shard group's committed tip is probed and recorded.
///
/// The latest recorded tip is what a verified read's anchor must be recent against, so this bounds
/// how far that reference trails the chain when no read refreshes it.
const COMMITTED_TIP_PROBE_INTERVAL: Duration = Duration::from_secs(10);
/// The most a peer may stream, in encoded bytes, for one state version before completing it. A
/// version is buffered whole and committed at once, so this bounds the memory a peer can hold to a
/// constant multiple of it: the buffered updates are held decoded, alongside what is derived from
/// them.
const MAX_BUFFERED_VERSION_BYTES: usize = 256 * 1024 * 1024;
// A state version is one block's writes to the shard, which consensus bounds by
// `MAX_BLOCK_VALIDATION_SHARD_OUTPUT_BYTES`. The stream carries each update with its metadata, so the
// buffer leaves twice that.
const _: () =
    assert!(MAX_BUFFERED_VERSION_BYTES >= 2 * tari_ootle_common_types::MAX_BLOCK_VALIDATION_SHARD_OUTPUT_BYTES);
/// How long a verified leaf may stay ahead of a shard its stream claims is level, or a staged version stay above every
/// verified leaf of its shard, before the stream peer is blamed. An honest peer streams a version within moments of its
/// committee committing it, and every probe asks for the leaf of each shard with versions staged above its last
/// verified one, so this leaves a few probes to fail.
const VERIFICATION_GRACE: Duration = COMMITTED_TIP_PROBE_INTERVAL.saturating_mul(3);
/// How often a stream checks for a peer to blame under [`VERIFICATION_GRACE`].
const VERIFICATION_CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// The soonest a shard group's tip is probed again after the last probe, when its stream closes off a burst of
/// versions. A burst's versions are promoted on the next leaf, so this bounds how far promotion trails the stream at
/// one probe per group a second.
const BURST_PROBE_MIN_INTERVAL: Duration = Duration::from_secs(1);
/// The most probes' leaves queued for a shard group's stream.
const PROBED_LEAVES_CAPACITY: usize = 4;
/// The most encoded bytes a stream stages above every verified leaf of its shards before it stops reading until a leaf
/// covers them. A single version may reach [`MAX_BUFFERED_VERSION_BYTES`], so this always admits one.
const MAX_UNCOVERED_BYTES: usize = 2 * MAX_BUFFERED_VERSION_BYTES;
/// The most versions a stream stages above every verified leaf of its shards before it stops reading until a leaf
/// covers them.
const MAX_UNCOVERED_VERSIONS: usize = 200_000;
/// The most stored checkpoint leaves of a shard checked at once above its verified version.
const CHECKPOINT_LEAVES_AHEAD: usize = 4;
/// The most staged versions promoted in one write transaction.
const MAX_PROMOTED_VERSIONS: usize = 1024;
/// The most encoded bytes of staged versions promoted in one write transaction, which bounds what it decodes and
/// announces at once.
const MAX_PROMOTED_BYTES: usize = 64 * 1024 * 1024;

/// The substate values a followed stream carries. The rest arrive as an id and a value hash, which with the values is
/// every transition: what the accumulator needs, and what lets the substate cache tell a superseded or destroyed entry
/// from a current one.
fn stream_value_filters() -> SubstateValueFilterFlags {
    SubstateValueFilterFlags::UTXO |
        SubstateValueFilterFlags::VALIDATOR_FEE_POOL |
        SubstateValueFilterFlags::CLAIMED_OUTPUT_TOMBSTONE |
        SubstateValueFilterFlags::TRANSACTION_RECEIPT |
        SubstateValueFilterFlags::TEMPLATE_METADATA |
        SubstateValueFilterFlags::ALL_HASHES
}

#[derive(Clone)]
pub struct NetworkWideStateSync {
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
    network: Network,
    /// Each streamed shard's transition accumulator, from its last verified version through the versions staged above
    /// it, checked against verified leaves. A shard's stage is restored from the store at the start of every round of
    /// its group.
    verified_stage: Arc<Mutex<VerifiedStage<PeerAddress>>>,
    #[cfg(feature = "metrics")]
    metrics: NetworkStateMetrics,
    #[cfg(feature = "metrics")]
    substate_cache_metrics: SubstateCacheMetrics,
    #[cfg(feature = "metrics")]
    substate_manager: SubstateManager,
}

impl NetworkWideStateSync {
    pub fn new(
        epoch_manager: EpochManagerHandle<PeerAddress>,
        networking: NetworkingHandle<TariMessagingSpec>,
        storage: SqliteIndexerStore,
        config: NetworkWideStateSyncConfig,
        notify: Notify<IndexerEvent>,
        transaction_event_notify: Notify<TransactionEvent>,
        validator_status: ValidatorStatusMonitor,
        shard_watermarks: Arc<ShardWatermarks>,
        consensus_epoch: ConsensusEpoch,
        network: Network,
        #[cfg(feature = "metrics")] metrics: NetworkStateMetrics,
        #[cfg(feature = "metrics")] substate_cache_metrics: SubstateCacheMetrics,
        #[cfg(feature = "metrics")] substate_manager: SubstateManager,
    ) -> Self {
        Self {
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
            network,
            verified_stage: Arc::default(),
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
        tokio::spawn(self.clone().report(shutdown_signal.clone()));
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
                // While `sync` is pending, a branch body awaits only `sync` itself or work that is
                // independent of the indexer store. `sync` goes unpolled while a body runs, so a write
                // lock handed to it then is never taken, and every writer queues behind it. The epoch
                // manager keeps its state outside the indexer store.
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
                }
            }
        }
    }

    /// Logs and resets the sync stats, and publishes the metrics, every work interval until shutdown.
    ///
    /// Reporting runs on a task of its own: a metrics refresh can write the substate cache, which
    /// queues on the store's write lock behind the sync's own commits.
    async fn report(self, mut shutdown_signal: ShutdownSignal) {
        let reporting = async {
            if let Err(err) = self.epoch_manager.wait_for_initial_scanning_to_complete().await {
                warn!(target: LOG_TARGET, "⚠️ State sync stats are not reported: the epoch manager failed its initial scan: {err}");
                return;
            }

            // Publish last-known totals immediately so a freshly restarted indexer reports them before
            // its first sync round completes.
            #[cfg(feature = "metrics")]
            self.update_metrics().await;

            let mut report_interval = time::interval(self.config.work_interval);
            report_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
            report_interval.reset();
            loop {
                report_interval.tick().await;
                self.stats.log_stats();
                self.stats.reset();
                #[cfg(feature = "metrics")]
                self.update_metrics().await;
            }
        };
        tokio::select! {
            _ = shutdown_signal.wait() => {},
            _ = reporting => {},
        }
    }

    /// Reads the persisted economic totals and publishes them to the Prometheus gauges. Metrics are
    /// observability only, so a read failure is logged and the next report tries again.
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

    /// Syncs `shard_group`'s checkpoints up to the epoch before `epoch`, from wherever it left off, checking the shard
    /// leaves of each quorum-signed one against the staged versions of `shards`. Nothing to do once they are
    /// recorded, so this is run before every stream the group opens.
    ///
    /// Returns whether the group's checkpoints reach the epoch before `epoch`. They fall short when
    /// the committee has not yet completed that epoch: consensus ends an epoch some time after the
    /// base layer moves past it. They fall short for all of `epoch` when the epoch before it has no
    /// checkpoint at all, as in a network's first epoch or after an epoch consensus never ran. They
    /// also fall short when `cancel` stops the backfill between pages.
    async fn sync_group_checkpoints(
        &self,
        shard_group: ShardGroup,
        shards: &[Shard],
        pool: &mut ValidatorCommitteeRpcPool,
        epoch: Epoch,
        progress: &SharedSyncProgress,
        cancel: &CancellationToken,
    ) -> Result<bool, NetworkStateSyncError> {
        let Some(prev_epoch) = epoch.checked_sub(Epoch(1)) else {
            return Ok(true);
        };
        let checkpoint_epoch = || async { progress.lock().await.checkpoint_epoch(shard_group) };
        let mut from_epoch = checkpoint_epoch().await.unwrap_or_else(Epoch::zero);
        if from_epoch >= prev_epoch {
            debug!(target: LOG_TARGET, "🌍️ No checkpoints to sync for shard group {shard_group} from epoch {from_epoch}");
            return Ok(true);
        }
        if is_backfill(from_epoch, prev_epoch) {
            match self
                .sync_head_checkpoint(shard_group, shards, pool, prev_epoch, progress)
                .await
            {
                Ok(()) => {},
                Err(err) if err.is_peer_fault() && !err.is_unverified_history() => {
                    warn!(target: LOG_TARGET, "⚠️ Syncing the checkpoint of {prev_epoch} for shard group {shard_group} ahead of the backfill failed: {err}");
                },
                Err(err) => return Err(err),
            }
        }
        // Each page is fetched as soon as the one before it is stored, so a wallet walking from an older set waits
        // only as long as the pages take to verify. A cancelled plan stops between pages; progress is recorded per
        // checkpoint, so the next plan resumes where this one stopped.
        loop {
            info!(target: LOG_TARGET, "🌍️ Syncing checkpoints from {from_epoch} for shard group {shard_group}");
            let checkpoints = self
                .fetch_group_checkpoints(shard_group, pool, from_epoch, prev_epoch)
                .await?;

            let level = if checkpoints.is_empty() {
                Level::Debug
            } else {
                Level::Info
            };
            log!(target: LOG_TARGET, level, "🌍️ Found {} checkpoints for shard group {shard_group} from epoch {from_epoch} (prev_epoch {prev_epoch})", checkpoints.len());

            for checkpoint in checkpoints {
                let checkpoint_epoch = checkpoint.epoch();
                let leaves = self
                    .store_checkpoint(shard_group, checkpoint, prev_epoch, Some(progress))
                    .await?;
                self.apply_checkpoint_leaves(shards, leaves, checkpoint_epoch, progress)
                    .await?;
            }
            let reached = checkpoint_epoch().await;
            match reached {
                Some(reached) if reached >= prev_epoch => return Ok(true),
                Some(reached) if reached > from_epoch && !cancel.is_cancelled() => from_epoch = reached,
                _ => return Ok(false),
            }
        }
    }

    /// Syncs `shard_group`'s checkpoint of `prev_epoch` ahead of the backfill that leads up to it.
    ///
    /// A wallet verifies the indexer's reads at the current epoch only through this checkpoint, which hands over to the
    /// current epoch's validator set, so serving it first makes reads verifiable while older epochs are still being
    /// backfilled. The backfill's progress stays where it is.
    async fn sync_head_checkpoint(
        &self,
        shard_group: ShardGroup,
        shards: &[Shard],
        pool: &mut ValidatorCommitteeRpcPool,
        prev_epoch: Epoch,
        progress: &SharedSyncProgress,
    ) -> Result<(), NetworkStateSyncError> {
        // The checkpoint of `prev_epoch` is of that epoch's shard groups, which differ from `shard_group` when the
        // number of committees changed, so it is found by the shard the group starts at.
        let stored = self
            .store
            .with_read_tx(move |tx| tx.epoch_checkpoint_get_all(prev_epoch, Some(shard_group.start()), 1))
            .await?
            .first()
            .is_some_and(|checkpoint| checkpoint.epoch() == prev_epoch);
        if stored {
            return Ok(());
        }
        info!(target: LOG_TARGET, "🌍️ Syncing the checkpoint of {prev_epoch} for shard group {shard_group} ahead of the backfill");
        let checkpoints = self
            .fetch_group_checkpoints(shard_group, pool, prev_epoch, prev_epoch)
            .await?;
        for checkpoint in checkpoints {
            let leaves = self.store_checkpoint(shard_group, checkpoint, prev_epoch, None).await?;
            self.apply_checkpoint_leaves(shards, leaves, prev_epoch, progress)
                .await?;
        }
        Ok(())
    }

    /// `shard_group`'s checkpoints from `from_epoch` to `prev_epoch`, from a committee member whose committed tip
    /// verifies.
    async fn fetch_group_checkpoints(
        &self,
        shard_group: ShardGroup,
        pool: &mut ValidatorCommitteeRpcPool,
        from_epoch: Epoch,
        prev_epoch: Epoch,
    ) -> Result<Vec<EpochCheckpoint>, NetworkStateSyncError> {
        let validator_status = self.validator_status.clone();
        let consensus_epoch = self.consensus_epoch.clone();
        let mut checkpoints = pool
            .try_with_random_members(|mut session| {
                let validator_status = validator_status.clone();
                let consensus_epoch = consensus_epoch.clone();
                async move {
                    // Verify how far this peer has committed before trusting it as a sync source.
                    // `probe` only returns Err for a forged/malformed proof (other failures are
                    // logged internally and return Ok(None)), which disqualifies the peer so
                    // another committee member is tried.
                    match validator_status
                        .probe(&mut session, shard_group, &BTreeSet::new())
                        .await
                    {
                        Ok(Some(probe)) => consensus_epoch.observe(probe.tip.shard_group, probe.tip.epoch),
                        Ok(None) => {},
                        Err(e) => {
                            return Err(NetworkStateSyncError::InvalidCommitProof {
                                details: format!("shard group {shard_group}: {e}"),
                            });
                        },
                    }
                    fetch_checkpoints(&mut session, shard_group, from_epoch).await
                }
            })
            .await?;
        retain_checkpoints_in_sync_range(&mut checkpoints, from_epoch, prev_epoch);
        Ok(checkpoints)
    }

    /// Verifies `checkpoint` and stores it, advancing `shard_group`'s checkpoint progress to its epoch when `progress`
    /// is given. Returns the shard leaves of a quorum-signed checkpoint; one checked for form only carries shard
    /// states its sender chose, and has none.
    async fn store_checkpoint(
        &self,
        shard_group: ShardGroup,
        checkpoint: EpochCheckpoint,
        prev_epoch: Epoch,
        progress: Option<&SharedSyncProgress>,
    ) -> Result<Vec<VerifiedShardLeaf>, NetworkStateSyncError> {
        info!(target: LOG_TARGET, "🌍️ Validating checkpoint for shard group {shard_group}: {}", checkpoint.header().calculate_hash());

        let checkpoint_shard_group =
            checkpoint
                .checked_shard_group()
                .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                    details: format!("Checkpoint for shard group {} is not valid: {}", shard_group, e),
                })?;

        let checked = verify_checkpoint(
            self.network,
            &self.epoch_manager,
            &checkpoint,
            checkpoint_shard_group,
            prev_epoch,
        )
        .await?;

        info!(target: LOG_TARGET, "🌍️ Inserting checkpoint for {}, shard group {}", checkpoint.epoch(), checkpoint_shard_group);

        let xtr_exhausted = checkpoint_exhaust_burn(&checkpoint)?;
        let leaves = if checked == CheckpointCheck::QuorumSigned {
            VerifiedShardLeaf::from_checkpoint(&checkpoint).collect()
        } else {
            Vec::new()
        };
        self.stats.increment_checkpoints();
        let checkpoint_epoch = checkpoint.epoch();
        // The progress lock is held until the snapshot is written, as `SharedSyncProgress` requires.
        let mut progress = match progress {
            Some(progress) => Some(progress.lock().await),
            None => None,
        };
        let sync_progress_snapshot = progress.as_deref_mut().map(|progress| {
            progress.record_checkpoint(shard_group, checkpoint_epoch);
            progress.clone()
        });
        let stored_leaves = leaves
            .iter()
            .map(|leaf| CheckpointShardLeaf {
                shard: leaf.shard,
                state_version: leaf.state_version,
                transition_acc: leaf.transition_acc,
            })
            .collect::<Vec<_>>();
        self.store
            .with_write_tx(move |tx| {
                write_checkpoint(
                    tx,
                    &checkpoint,
                    checkpoint_shard_group,
                    xtr_exhausted,
                    &stored_leaves,
                    sync_progress_snapshot,
                )
            })
            .await?;
        Ok(leaves)
    }

    /// Checks the checkpoint leaves of `shards` against their staged versions.
    async fn apply_checkpoint_leaves(
        &self,
        shards: &[Shard],
        leaves: Vec<VerifiedShardLeaf>,
        epoch: Epoch,
        progress: &SharedSyncProgress,
    ) -> Result<(), NetworkStateSyncError> {
        for leaf in leaves.into_iter().filter(|leaf| shards.contains(&leaf.shard)) {
            self.apply_leaf(leaf, format!("the checkpoint of {epoch}"), progress)
                .await?;
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
        let mut probes = FuturesUnordered::new();
        let mut groups = FuturesUnordered::new();
        for (i, (shard_group, pool)) in committee_pools.into_iter().enumerate() {
            let (leaves_tx, leaves_rx) = mpsc::channel(PROBED_LEAVES_CAPACITY);
            let burst = Arc::new(tokio_sync::Notify::new());
            probes.push(self.probe_group_tips(*shard_group, pool.clone(), i == 0, leaves_tx, burst.clone()));
            groups.push(self.clone().follow_shard_group(
                *shard_group,
                pool.clone(),
                i == 0,
                sync_plan.sync_progress().clone(),
                cancel.clone(),
                epoch.clone(),
                GroupLeaves {
                    probed: leaves_rx,
                    burst,
                },
            ));
        }

        let followed = async {
            while let Some(result) = groups.next().await {
                result?;
            }
            Ok(())
        };
        let probing = async {
            match probes.next().await {
                Some(never) => match never {},
                None => std::future::pending().await,
            }
        };
        // The groups wind down on `cancel`; the probe loops are dropped once they have.
        tokio::select! {
            result = followed => result,
            never = probing => match never {},
        }
    }

    /// Probes `shard_group`'s committed tip each [`COMMITTED_TIP_PROBE_INTERVAL`], and again when `burst` signals that
    /// the group's stream closed off a burst of versions, no sooner than [`BURST_PROBE_MIN_INTERVAL`] after the last
    /// probe. Records each verified tip in the verified-root store, observes its epoch, and hands its shard leaves to
    /// the group's stream through `leaves`.
    ///
    /// A burst probe asks for the leaves of the shards with versions staged above their last verified one, which it
    /// promotes. A periodic probe asks for every shard's leaf, so that a shard the stream claims is level but whose
    /// committee has moved on is found waiting for a leaf, which blames the stream peer for withholding it.
    ///
    /// A followed stream only probes when it reopens, which on a quiet network is up to a stream
    /// deadline away and on a busy one may be never. Probing here keeps [`ConsensusEpoch`] following a
    /// committee into a new epoch soon after its end-of-epoch block, keeps the latest recorded
    /// tip, which a verified read's anchor must be recent against, near the chain's, and supplies the
    /// leaves the stream's staged versions are promoted on.
    async fn probe_group_tips(
        &self,
        shard_group: ShardGroup,
        mut pool: ValidatorCommitteeRpcPool,
        syncs_global_shard: bool,
        leaves: mpsc::Sender<Vec<VerifiedShardLeaf>>,
        burst: Arc<tokio_sync::Notify>,
    ) -> Infallible {
        let mut interval = time::interval(COMMITTED_TIP_PROBE_INTERVAL);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        let mut last_probe = None::<time::Instant>;
        let all_shards = syncs_global_shard
            .then_some(Shard::global())
            .into_iter()
            .chain(shard_group.shard_iter())
            .collect::<BTreeSet<_>>();
        loop {
            let periodic = tokio::select! {
                _ = interval.tick() => true,
                () = burst.notified() => {
                    if let Some(last_probe) = last_probe {
                        time::sleep_until(last_probe + BURST_PROBE_MIN_INTERVAL).await;
                    }
                    false
                },
            };
            last_probe = Some(time::Instant::now());
            let mut session = match pool.new_session().await {
                Ok(session) => session,
                Err(e) => {
                    debug!(target: LOG_TARGET, "No session to probe the committed tip of shard group {shard_group}: {e}");
                    continue;
                },
            };
            let leaf_shards = if periodic {
                all_shards.clone()
            } else {
                self.leaf_shards(shard_group, syncs_global_shard)
            };
            match self
                .validator_status
                .probe(&mut session, shard_group, &leaf_shards)
                .await
            {
                Ok(Some(probe)) => {
                    self.consensus_epoch.observe(probe.tip.shard_group, probe.tip.epoch);
                    // A full queue drops these leaves: the stream has yet to take the ones queued before them, and
                    // the next probe asks again.
                    if !probe.leaves.is_empty() && leaves.try_send(probe.leaves).is_err() {
                        debug!(target: LOG_TARGET, "The stream of shard group {shard_group} has not taken the leaves of earlier probes. Dropped this probe's");
                    }
                    if let Err(e) = self.persist_verified_tip(probe.tip).await {
                        warn!(target: LOG_TARGET, "⚠️ Failed to record verified state root for shard group {shard_group}: {e}");
                    }
                },
                Ok(None) => {},
                Err(e) => {
                    warn!(target: LOG_TARGET, "⚠️ Validator {} for shard group {} served an INVALID commit proof: {}", session.peer_address(), shard_group, e);
                },
            }
        }
    }

    /// Keeps one shard group synced: restores its shards' verified stage, syncs its checkpoints,
    /// opens a stream from a committee member, follows it until it ends, and goes round again,
    /// until cancelled.
    ///
    /// A stream that ends because the validator had nothing to send for the deadline is reopened at
    /// once - that is the ordinary end of a followed stream, and the reopen refreshes each shard's
    /// watermark. So is one wound down because the epoch moved: the next round syncs the new
    /// checkpoints and resolves the committee at the new epoch. One closed by a validator that does
    /// not follow, or failed by the peer, waits the work interval first: the former is polling, the
    /// latter wants a different peer. A peer whose history no verified leaf covers is excluded for
    /// the rest of the epoch, and the next round reopens at once with another member. Once every
    /// member's history has mismatched from the same verified version, the group halts until the
    /// epoch moves.
    #[expect(clippy::too_many_arguments)]
    async fn follow_shard_group(
        mut self,
        shard_group: ShardGroup,
        mut pool: ValidatorCommitteeRpcPool,
        syncs_global_shard: bool,
        progress: SharedSyncProgress,
        cancel: CancellationToken,
        mut epoch: watch::Receiver<Epoch>,
        mut leaves: GroupLeaves,
    ) -> Result<(), NetworkStateSyncError> {
        // Shard 0 sorts before every preshard, which keeps the cursor list ascending as the
        // responder requires.
        let shards = syncs_global_shard
            .then_some(Shard::global())
            .into_iter()
            .chain(shard_group.shard_iter())
            .collect::<Vec<_>>();
        let mut breaker = MismatchBreaker::default();
        while !cancel.is_cancelled() {
            let round = self
                .follow_round(
                    shard_group,
                    &shards,
                    &mut pool,
                    &progress,
                    &cancel,
                    &mut epoch,
                    &mut leaves,
                )
                .await;
            match round {
                Ok(RoundEnd::Cancelled) => return Ok(()),
                Ok(RoundEnd::Reopen) => {},
                Ok(RoundEnd::Pause(reason)) => self.pause(shard_group, reason, &cancel, &mut epoch).await,
                Err(NetworkStateSyncError::UnverifiedHistory {
                    details,
                    blamed,
                    diverged_from,
                }) => {
                    warn!(target: LOG_TARGET, "⚠️ Shard group {shard_group}: {details}. Excluding {} for the rest of the epoch", display_peers(&blamed));
                    for peer in &blamed {
                        pool.mark_failed(*peer);
                    }
                    let Some((shard, from)) = diverged_from else {
                        continue;
                    };
                    let committee_size = match pool.committee_size().await {
                        Ok(committee_size) => committee_size,
                        Err(e) => {
                            warn!(target: LOG_TARGET, "⚠️ Shard group {shard_group}'s committee size is unknown, so its divergent history is not counted towards halting it: {e}");
                            continue;
                        },
                    };
                    if breaker.record(shard, from, &blamed, committee_size) {
                        error!(target: LOG_TARGET, "⚠️ Every one of the {committee_size} members of shard group {shard_group} streamed history of shard {shard} after v{from} that mismatches a verified leaf or cannot be accumulated. This indexer's own accumulator recomputation is suspect, so it stops following the shard group until the epoch moves. Report this with the errors above");
                        #[cfg(feature = "metrics")]
                        self.metrics.record_leaf_check("circuit_breaker");
                        breaker = MismatchBreaker::default();
                        tokio::select! {
                            _ = cancel.cancelled() => {},
                            Ok(()) = epoch.changed() => {},
                        }
                    }
                },
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    /// One round of [`Self::follow_shard_group`]: restores the stage, syncs checkpoints, probes, and follows a stream
    /// until it ends. A failure the peer is at fault for ends the round in a pause, except one that blames the peer for
    /// unverified history, which is returned.
    #[expect(clippy::too_many_arguments)]
    async fn follow_round(
        &mut self,
        shard_group: ShardGroup,
        shards: &[Shard],
        pool: &mut ValidatorCommitteeRpcPool,
        progress: &SharedSyncProgress,
        cancel: &CancellationToken,
        epoch: &mut watch::Receiver<Epoch>,
        leaves: &mut GroupLeaves,
    ) -> Result<RoundEnd, NetworkStateSyncError> {
        let current_epoch = *epoch.borrow_and_update();
        self.restore_stage(shards, progress).await?;
        let checkpoints_reached_epoch = match self
            .sync_group_checkpoints(shard_group, shards, pool, current_epoch, progress, cancel)
            .await
        {
            Ok(reached) => reached,
            Err(err) if err.is_peer_fault() && !err.is_unverified_history() => {
                warn!(target: LOG_TARGET, "⚠️ Checkpoint sync for shard group {} failed: {}", shard_group, err);
                return Ok(RoundEnd::Pause("checkpoint sync failed"));
            },
            Err(err) => return Err(err),
        };
        if cancel.is_cancelled() {
            return Ok(RoundEnd::Cancelled);
        }
        // Wallets verify the validator set of an epoch through the checkpoint that ends the epoch
        // before it, so a missing checkpoint is fetched again after the work interval.
        let checkpoints_due_at = (!checkpoints_reached_epoch).then(|| time::Instant::now() + self.config.work_interval);
        let mut session = match pool.new_stream_session().await {
            Ok(s) => s,
            Err(e) => {
                warn!(target: LOG_TARGET, "⚠️ Failed to create session for shard group {}: {}", shard_group, e);
                return Ok(RoundEnd::Pause("no session"));
            },
        };
        let leaf_shards = self.leaf_shards(shard_group, shards.contains(&Shard::global()));
        match self
            .validator_status
            .probe(&mut session, shard_group, &leaf_shards)
            .await
        {
            Ok(Some(probe)) => {
                self.consensus_epoch.observe(probe.tip.shard_group, probe.tip.epoch);
                // Record the quorum-signed state root so the read path can skip re-validating
                // commit proofs for this tip. A failure here must not abort the state sync.
                if let Err(e) = self.persist_verified_tip(probe.tip).await {
                    warn!(target: LOG_TARGET, "⚠️ Failed to record verified state root for shard group {}: {}", shard_group, e);
                }
                let peer = *session.peer_address();
                for leaf in probe.leaves {
                    self.apply_leaf(leaf, format!("the tip leaf from {peer}"), progress)
                        .await?;
                }
            },
            Ok(None) => {},
            // probe only returns Err for an invalid (forged) commit proof.
            Err(e) => {
                warn!(target: LOG_TARGET, "⚠️ Validator {} for shard group {} served an INVALID commit proof: {}", session.peer_address(), shard_group, e);
                return Ok(RoundEnd::Pause("invalid commit proof"));
            },
        }

        // A responder that cannot serve these shards - it left the committee, or the epoch this
        // indexer resolved its committees at has moved on - costs this shard group a retry and
        // no more.
        let end = match self
            .sync_shard_group_state(
                shards,
                progress,
                shard_group,
                &mut session,
                cancel,
                epoch,
                checkpoints_due_at,
                leaves,
            )
            .await
        {
            Ok(end) => end,
            Err(err) if err.is_peer_fault() && !err.is_unverified_history() => {
                warn!(target: LOG_TARGET, "⚠️ State sync for shard group {} from {} failed: {}", shard_group, session.peer_address(), err);
                return Ok(RoundEnd::Pause("the peer failed"));
            },
            Err(err) => return Err(err),
        };
        Ok(match end {
            StreamEnd::Cancelled => RoundEnd::Cancelled,
            StreamEnd::TimedOut => {
                debug!(target: LOG_TARGET, "🌍️ State sync stream for shard group {shard_group} from {} had nothing to send for the deadline. Reopening", session.peer_address());
                RoundEnd::Reopen
            },
            StreamEnd::EpochAdvanced => {
                info!(target: LOG_TARGET, "🌍️ Epoch advanced to {}. Reopening state sync for shard group {shard_group}", *epoch.borrow());
                RoundEnd::Reopen
            },
            StreamEnd::CheckpointsDue => {
                info!(target: LOG_TARGET, "🌍️ Shard group {shard_group} has no checkpoint for the epoch before {current_epoch} yet. Syncing checkpoints again");
                RoundEnd::Reopen
            },
            StreamEnd::Final => RoundEnd::Pause("the validator closed the stream at its tip and does not follow"),
        })
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

    /// Follows every given shard from `session`, which serves them all over a single stream, from the version after
    /// the last one staged. A shard never synced follows from version one.
    #[expect(clippy::too_many_arguments)]
    async fn sync_shard_group_state(
        &mut self,
        shards: &[Shard],
        progress: &SharedSyncProgress,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
        cancel: &CancellationToken,
        epoch: &mut watch::Receiver<Epoch>,
        checkpoints_due_at: Option<time::Instant>,
        leaves: &mut GroupLeaves,
    ) -> Result<StreamEnd, NetworkStateSyncError> {
        let cursors = cursors_for(shards, &*progress.lock().await);
        if cursors.is_empty() {
            warn!(target: LOG_TARGET, "⚠️ Every shard in shard group {shard_group} is recorded at the last representable state version. Nothing to follow");
            return Ok(StreamEnd::Final);
        }
        self.stream_shard_state(
            cursors,
            progress,
            shard_group,
            session,
            cancel,
            epoch,
            checkpoints_due_at,
            leaves,
        )
        .await
    }

    /// The shards of `shard_group` whose leaf the next probe asks for: those a leaf would verify.
    fn leaf_shards(&self, shard_group: ShardGroup, syncs_global_shard: bool) -> BTreeSet<Shard> {
        let stage = self.verified_stage();
        syncs_global_shard
            .then_some(Shard::global())
            .into_iter()
            .chain(shard_group.shard_iter())
            .filter(|shard| stage.wants_leaf(*shard))
            .collect()
    }

    /// Resets the verified stage of `shards` to their last verified versions and the versions staged above them, as
    /// the store holds them, and acts on the verdict of a leaf a shard was waiting for.
    async fn restore_stage(
        &self,
        shards: &[Shard],
        progress: &SharedSyncProgress,
    ) -> Result<(), NetworkStateSyncError> {
        let recorded = {
            let progress = progress.lock().await;
            shards
                .iter()
                .map(|&shard| (shard, progress.verified(shard), progress.last_state_version(shard)))
                .collect::<Vec<_>>()
        };
        let tail_shards = shards.to_vec();
        let tails = self
            .store
            .with_read_tx(move |tx| {
                tail_shards
                    .into_iter()
                    .map(|shard| tx.staged_accumulators_tail(shard, MAX_ACCUMULATOR_HISTORY))
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
        for ((shard, verified, cursor), tail) in recorded.into_iter().zip(tails) {
            let head = tail.last().map_or(verified.version, |staged| staged.state_version);
            if cursor.unwrap_or(StateVersion::new(0)) != head {
                return Err(NetworkStateSyncError::InvariantError {
                    details: format!(
                        "shard {shard}'s stream resumes after v{}, but its last verified or staged version is v{head}",
                        cursor.unwrap_or(StateVersion::new(0))
                    ),
                });
            }
            let staged = tail
                .into_iter()
                .map(|staged| {
                    Ok(Staged {
                        version: staged.state_version,
                        epoch: staged.epoch,
                        acc: staged.transition_acc,
                        streamed_by: decode_peer(&staged.streamed_by)?,
                    })
                })
                .collect::<Result<Vec<_>, NetworkStateSyncError>>()?;
            let verdict = self.verified_stage().restore(shard, verified, staged).map_err(|e| {
                NetworkStateSyncError::InvariantError {
                    details: format!("the staged versions of shard {shard} do not restore: {e}"),
                }
            })?;
            let (verdict, checked_on) = match verdict {
                Some(verdict) => (Some(verdict), "the leaf it was waiting for"),
                None => (
                    self.check_stored_leaves(shard, progress).await?,
                    "a stored checkpoint leaf",
                ),
            };
            if let Some(verdict) = verdict {
                self.act_on_verdict(shard, verdict, checked_on, progress).await?;
            }
        }
        Ok(())
    }

    /// Checks `leaf` against its shard's staged versions and acts on the verdict. Returns the version promoted to.
    async fn apply_leaf(
        &self,
        leaf: VerifiedShardLeaf,
        checked_on: impl Display,
        progress: &SharedSyncProgress,
    ) -> Result<Option<StateVersion>, NetworkStateSyncError> {
        let verdict = self.verified_stage().on_leaf(leaf);
        self.act_on_verdict(leaf.shard, verdict, checked_on, progress).await
    }

    /// Logs and counts `verdict` on `shard`, then promotes the staged versions it authenticates, or discards those
    /// staged above the shard's last verified version when it mismatches. A mismatch is returned as an error that
    /// blames every peer whose staged version was discarded. Returns the version promoted to.
    ///
    /// Each promotion moves the verified version past the stored checkpoint leaves checked so far, so the next ones
    /// above it are checked in turn.
    async fn act_on_verdict(
        &self,
        shard: Shard,
        verdict: Verdict<PeerAddress>,
        checked_on: impl Display,
        progress: &SharedSyncProgress,
    ) -> Result<Option<StateVersion>, NetworkStateSyncError> {
        let mut verdict = verdict;
        let mut checked_on = checked_on.to_string();
        let mut promoted = None;
        loop {
            match verdict {
                Verdict::Promote(version) => {
                    debug!(target: LOG_TARGET, "🌍️ Shard {shard} verified to v{version} by {checked_on}");
                    self.record_leaf_check("promote");
                    self.promote(shard, version, progress).await?;
                    promoted = Some(version);
                    match self.check_stored_leaves(shard, progress).await? {
                        Some(next) => {
                            verdict = next;
                            checked_on = "a stored checkpoint leaf".to_string();
                        },
                        None => return Ok(promoted),
                    }
                },
                Verdict::Mismatch {
                    version,
                    epoch,
                    computed,
                    verified,
                    streamed_by,
                } => {
                    self.record_leaf_check("mismatch");
                    let (rewound_to, blamed) = self.discard_staged(shard, progress).await?;
                    return Err(NetworkStateSyncError::UnverifiedHistory {
                        details: format!(
                            "Transition accumulator mismatch: shard {shard} v{version} at {epoch} streamed by \
                             {streamed_by} computes {computed}, but the verified leaf has {verified} (checked on \
                             {checked_on}). Discarded every version staged above v{rewound_to}"
                        ),
                        blamed,
                        diverged_from: Some((shard, rewound_to)),
                    });
                },
                Verdict::Conflict {
                    version,
                    held,
                    verified,
                } => {
                    error!(target: LOG_TARGET, "⚠️ Verified leaves conflict: shard {shard} v{version} was verified at transition accumulator {held}, but {checked_on} has {verified}");
                    self.record_leaf_check("conflict");
                    return Ok(promoted);
                },
                Verdict::Wait | Verdict::Stale => return Ok(promoted),
            }
        }
    }

    /// Checks the first [`CHECKPOINT_LEAVES_AHEAD`] stored checkpoint leaves above `shard`'s verified version,
    /// returning the verdict of the last that checks anything, or of the first that mismatches. Those ahead of the
    /// stream wait for it.
    async fn check_stored_leaves(
        &self,
        shard: Shard,
        progress: &SharedSyncProgress,
    ) -> Result<Option<Verdict<PeerAddress>>, NetworkStateSyncError> {
        let verified = progress.lock().await.verified(shard).version;
        let leaves = self
            .store
            .with_read_tx(move |tx| tx.checkpoint_shard_leaves_above(shard, verified, CHECKPOINT_LEAVES_AHEAD))
            .await?;
        let mut stage = self.verified_stage();
        let mut checked = None;
        for leaf in leaves {
            let verdict = stage.on_leaf(VerifiedShardLeaf {
                shard: leaf.shard,
                state_version: leaf.state_version,
                transition_acc: leaf.transition_acc,
            });
            match verdict {
                mismatch @ Verdict::Mismatch { .. } => return Ok(Some(mismatch)),
                Verdict::Wait | Verdict::Stale => {},
                verdict => checked = Some(verdict),
            }
        }
        Ok(checked)
    }

    fn record_leaf_check(&self, outcome: &'static str) {
        #[cfg(feature = "metrics")]
        self.metrics.record_leaf_check(outcome);
        #[cfg(not(feature = "metrics"))]
        let _ = outcome;
    }

    /// Promotes `shard`'s staged versions up to `through` to the indexed tables, a write transaction at a time, and
    /// announces what each wrote once it commits.
    async fn promote(
        &self,
        shard: Shard,
        through: StateVersion,
        progress: &SharedSyncProgress,
    ) -> Result<(), NetworkStateSyncError> {
        loop {
            // The progress lock is held until the snapshot is written, as `SharedSyncProgress` requires.
            let mut progress = progress.lock().await;
            let from = progress.verified(shard).version;
            if from >= through {
                return Ok(());
            }
            let snapshot = progress.clone();
            let event_filters = self.config.event_filters.clone();
            let watched_templates = self.config.watched_templates.clone();
            let (verified, promoted) = self
                .store
                .with_write_tx(move |tx| {
                    promote_staged(tx, shard, from, through, snapshot, &event_filters, &watched_templates)
                })
                .await?;
            progress.record_verified(shard, verified);
            drop(progress);
            self.announce(shard, promoted);
        }
    }

    /// Discards every version of `shard` staged above its last verified one, moving its stream cursor back to it.
    /// Returns the verified version and the peers that streamed what was discarded.
    async fn discard_staged(
        &self,
        shard: Shard,
        progress: &SharedSyncProgress,
    ) -> Result<(StateVersion, Vec<PeerAddress>), NetworkStateSyncError> {
        // The progress lock is held until the snapshot is written, as `SharedSyncProgress` requires.
        let mut progress = progress.lock().await;
        let verified = progress.verified(shard).version;
        progress.rewind_to_verified(shard);
        let snapshot = progress.clone();
        let peers = self
            .store
            .with_write_tx(move |tx| {
                let peers = tx.staged_versions_discard_above(shard, verified)?;
                tx.key_value_set(Key::SyncProgress, snapshot)?;
                Ok::<_, StorageError>(peers)
            })
            .await?;
        drop(progress);
        let peers = peers
            .iter()
            .map(|peer| decode_peer(peer))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((verified, peers))
    }

    /// Announces what promoting a shard's versions wrote, once its write transaction has committed.
    fn announce(&self, shard: Shard, promoted: PromotedVersions) {
        self.stats.increase_events(promoted.num_receipt_events);
        if promoted.retired_cache_entries > 0 {
            debug!(target: LOG_TARGET, "Retired {} cached substates for shard {shard}", promoted.retired_cache_entries);
            #[cfg(feature = "metrics")]
            self.substate_cache_metrics
                .add_invalidations(promoted.retired_cache_entries);
        }
        for (state_version, address) in &promoted.skipped_utxos {
            warn!(target: LOG_TARGET, "⚠️ Unspent output {address} at v{state_version} of shard {shard} is already indexed. Kept the indexed row");
        }
        for finalized in promoted.finalized {
            self.notify.notify(finalized);
        }
        for inserted in promoted.inserted_events {
            self.transaction_event_notify.notify(TransactionEvent {
                id: inserted.id,
                transaction_id: inserted.transaction_id,
                event: inserted.event,
                encoded: inserted.encoded,
            });
        }
    }

    fn verified_stage(&self) -> MutexGuard<'_, VerifiedStage<PeerAddress>> {
        self.verified_stage.lock().unwrap_or_else(PoisonError::into_inner)
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

    /// Consumes a followed `sync_state` stream covering `cursors`, staging each version it streams and promoting the
    /// staged versions each verified leaf authenticates.
    ///
    /// The responder streams each shard's updates contiguously and closes it off with a completion
    /// marker. Each version is staged with the transition accumulator computed through it as it
    /// completes, so an interrupted stream keeps everything already staged and simply resumes from
    /// the recorded cursors when reopened. A followed stream keeps going past the tip, closing off
    /// each burst of a shard's new versions with a further marker and asking for a probe of the
    /// group's tip; it ends when the responder has had nothing to send for the deadline, or can no
    /// longer serve it.
    ///
    /// The stream peer is blamed, ending the stream, when its history mismatches a verified leaf,
    /// when a verified leaf stays ahead of a shard it claims is level for [`VERIFICATION_GRACE`],
    /// or when a version it staged stays above every verified leaf of its shard for as long. The
    /// stream stops being read while what it staged above every verified leaf reaches
    /// [`MAX_UNCOVERED_BYTES`] or [`MAX_UNCOVERED_VERSIONS`].
    ///
    /// Cancellation and an epoch change are honoured between messages, so a version being staged
    /// or promoted when either arrives is written in full and the recorded progress reflects it.
    #[expect(clippy::too_many_lines, clippy::too_many_arguments)]
    async fn stream_shard_state(
        &mut self,
        cursors: Vec<rpc::ShardCursor>,
        progress: &SharedSyncProgress,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
        cancel: &CancellationToken,
        epoch: &mut watch::Receiver<Epoch>,
        checkpoints_due_at: Option<time::Instant>,
        leaves: &mut GroupLeaves,
    ) -> Result<StreamEnd, NetworkStateSyncError> {
        let value_filters = stream_value_filters();
        let mut order = StreamOrder::new(&cursors);
        let peer = *session.peer_address();

        info!(
            target: LOG_TARGET,
            "🌍️ Following {} shard(s) in shard group {shard_group} from peer {peer}",
            cursors.len(),
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
                    follow: true,
                    include_version_proofs: false,
                },
                options,
            )
            .await?;

        // A keepalive says the responder is there and has nothing to send: for every shard it has
        // closed off on this stream, that is the claim a further marker would make, so each one
        // re-stamps those shards' watermarks. A shard not yet closed off, or streamed since its last
        // marker, is still being caught up and is not level, keepalive or not.
        let mut keepalives = stream.keepalives();
        let mut keepalives_open = true;
        let mut level = HashMap::<Shard, StateVersion>::new();
        let mut checks = StreamChecks::default();
        let mut check_interval = time::interval(VERIFICATION_CHECK_INTERVAL);
        check_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        check_interval.reset();

        // Buffers accumulate a single (shard, state version) at a time: the responder splits an
        // oversized version into chunks flagged `has_more`, and the last chunk flushes them.
        let mut num_updates = 0usize;
        let mut encoded_batches = Vec::new();
        let mut tree_changes = Ok(Vec::new());

        loop {
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(StreamEnd::Cancelled),
                Ok(()) = epoch.changed() => return Ok(StreamEnd::EpochAdvanced),
                () = time::sleep_until(checkpoints_due_at.unwrap_or_else(time::Instant::now)), if checkpoints_due_at.is_some() => {
                    return Ok(StreamEnd::CheckpointsDue);
                },
                Some(probed) = leaves.probed.recv() => {
                    for leaf in probed {
                        checks.cover(leaf.shard, leaf.state_version);
                        let promoted = self
                            .apply_leaf(leaf, format!("the tip leaf at v{}", leaf.state_version), progress)
                            .await?;
                        self.confirm_promoted(leaf.shard, promoted, &level);
                    }
                    self.check_stream_peer(&mut checks, &level, &peer, progress).await?;
                    continue;
                },
                _ = check_interval.tick() => {
                    self.check_stream_peer(&mut checks, &level, &peer, progress).await?;
                    continue;
                },
                next = stream.next(), if !checks.is_full() => match next {
                    Some(result) => result,
                    None => break,
                },
                changed = keepalives.changed(), if keepalives_open => {
                    match changed {
                        Ok(()) => {
                            let stage = self.verified_stage();
                            for (shard, level_at) in &level {
                                if stage.verified_version(*shard) >= *level_at {
                                    self.shard_watermarks.refresh(*shard);
                                }
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
                Err(status) if status.as_status_code().is_timeout() => {
                    debug!(target: LOG_TARGET, "🌍️ State sync stream for shard group {shard_group} timed out: {status}");
                    return Ok(StreamEnd::TimedOut);
                },
                Err(status) => return Err(status.into()),
            };
            let batch = match msg.response {
                Some(rpc::sync_state_response::Response::Batch(batch)) => batch,
                Some(rpc::sync_state_response::Response::VersionProof(_)) => {
                    return Err(NetworkStateSyncError::InvalidStateUpdate {
                        details: "Received a state version proof that was not requested".to_string(),
                    });
                },
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
                    // Recorded progress only ever advances with a staged version, never to the
                    // version a marker claims: a claim past what was streamed would skip every
                    // version in between for good.
                    let (level_at, burst) = order
                        .accept_marker(shard, synced_to)
                        .map_err(|details| NetworkStateSyncError::InvalidStateUpdate { details })?;
                    // The completion marker is the only point at which this shard is known to be
                    // level with the committee, which is what the substate cache needs: mid-stream
                    // the indexer holds every transition up to some version while the chain is
                    // arbitrarily far ahead of it. Confirmed even when the watermark did not move -
                    // a quiet shard is still one this indexer is keeping up with.
                    level.insert(shard, level_at);
                    self.confirm_level(shard, level_at);
                    if burst {
                        leaves.burst.notify_one();
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
            let encoded_len = batch.encoded_len();
            order
                .accept_batch(shard, state_version, batch.has_more, batch.updates.len(), encoded_len)
                .map_err(|details| NetworkStateSyncError::InvalidStateUpdate { details })?;
            level.remove(&shard);

            num_updates += batch.updates.len();
            encoded_batches.extend(batch.encode_length_delimited_to_vec());
            for update in batch.updates {
                let update =
                    SubstateUpdateProof::try_from(update).map_err(|e| NetworkStateSyncError::InvalidStateUpdate {
                        details: format!("Failed to convert substate update: {}", e),
                    })?;
                if let Ok(changes) = &mut tree_changes {
                    match streamed_tree_change(self.network, msg_epoch, &update) {
                        Ok(change) => changes.push(change),
                        Err(e) => tree_changes = Err(e),
                    }
                }
            }
            if batch.has_more {
                debug!(target: LOG_TARGET, "🌍️ more updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})");
                continue;
            }

            debug!(target: LOG_TARGET, "🌍️ Received {num_updates} updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})");
            self.stats.increase_state_updates(std::mem::take(&mut num_updates));

            let staged = self.stage_version(
                shard,
                state_version,
                msg_epoch,
                std::mem::replace(&mut tree_changes, Ok(Vec::new())),
                std::mem::take(&mut encoded_batches),
                &peer,
            )?;
            let staged_bytes = staged.batch.len();
            let (verdict, staged) = staged.into_parts();
            // The progress lock is held until the snapshot is written, as `SharedSyncProgress` requires.
            let mut progress_guard = progress.lock().await;
            progress_guard.record_state_version(shard, state_version, msg_epoch);
            let sync_progress_snapshot = progress_guard.clone();
            self.store
                .with_write_tx(move |tx| {
                    tx.staged_version_insert(shard, &staged)?;
                    tx.key_value_set(Key::SyncProgress, sync_progress_snapshot)
                })
                .await?;
            drop(progress_guard);
            if self
                .verified_stage()
                .highest_leaf(shard)
                .is_none_or(|leaf| leaf < state_version)
            {
                checks.stage(shard, state_version, staged_bytes, time::Instant::now());
            }
            if let Some(verdict) = verdict {
                let promoted = self
                    .act_on_verdict(shard, verdict, format!("v{state_version} streamed by {peer}"), progress)
                    .await?;
                self.confirm_promoted(shard, promoted, &level);
            }
        }

        // A followed stream is only ever ended by the responder for want of its warrant, which it
        // reports, or of consensus. Ending silently is the peer's failing either way.
        Err(NetworkStateSyncError::InvalidStateUpdate {
            details: format!("Followed state sync stream for shard group {shard_group} was closed by the responder"),
        })
    }

    /// Extends `shard`'s accumulator with the streamed `state_version`, whose transitions are `changes` and whose
    /// encoded chunks are `batch`, returning what to stage for it with the verdict on a leaf that was waiting for it.
    fn stage_version(
        &self,
        shard: Shard,
        state_version: StateVersion,
        epoch: Epoch,
        changes: Result<Vec<SubstateTreeChange>, StreamedTransitionError>,
        batch: Vec<u8>,
        peer: &PeerAddress,
    ) -> Result<StagedWithVerdict, NetworkStateSyncError> {
        // A version this indexer cannot accumulate counts towards the circuit breaker as a mismatch does: if every
        // member streams one, the recomputation is the likelier fault.
        let verified = self.verified_stage().verified_version(shard);
        let invalid = |details: String| {
            self.record_leaf_check("invalid_version");
            NetworkStateSyncError::UnverifiedHistory {
                details: format!("Peer {peer} streamed v{state_version} of shard {shard} at {epoch}, {details}"),
                blamed: vec![*peer],
                diverged_from: Some((shard, verified)),
            }
        };
        let changes = changes.map_err(|e| invalid(format!("which no committed version could hold: {e}")))?;
        let (transition_acc, verdict) = self
            .verified_stage()
            .on_version(shard, state_version, epoch, &changes, *peer)
            .map_err(|e| match e {
                e @ StageError::NotNext { .. } => NetworkStateSyncError::InvariantError {
                    details: format!("shard {shard}'s stream and its verified stage disagree: {e}"),
                },
                e @ StageError::Uncomputable { .. } => invalid(e.to_string()),
            })?;
        Ok(StagedWithVerdict {
            verdict,
            batch,
            accumulator: StagedAccumulator {
                state_version,
                epoch,
                transition_acc,
                streamed_by: peer.to_bytes(),
            },
        })
    }

    /// Confirms `shard`'s watermark at `level_at`, the version the stream found it level at, once every version up to
    /// it is promoted.
    ///
    /// The substate cache retires an entry only when the version that supersedes it is promoted, and serves on a
    /// confirmation's freshness alone, so a shard with versions still staged below its level is left to age.
    fn confirm_level(&self, shard: Shard, level_at: StateVersion) {
        if self.verified_stage().verified_version(shard) >= level_at {
            self.shard_watermarks.confirm(shard, level_at);
        }
    }

    /// Confirms `shard`'s watermark once a promotion reaches the version the stream last found it level at.
    fn confirm_promoted(&self, shard: Shard, promoted: Option<StateVersion>, level: &HashMap<Shard, StateVersion>) {
        if let (Some(_), Some(level_at)) = (promoted, level.get(&shard)) {
            self.confirm_level(shard, *level_at);
        }
    }

    /// Blames the stream peer for a verified leaf it withholds, or for a version it staged that no verified leaf has
    /// reached, for longer than [`VERIFICATION_GRACE`]. What was staged above the last verified version of a shard
    /// with an unreached version is discarded.
    async fn check_stream_peer(
        &self,
        checks: &mut StreamChecks,
        level: &HashMap<Shard, StateVersion>,
        peer: &PeerAddress,
        progress: &SharedSyncProgress,
    ) -> Result<(), NetworkStateSyncError> {
        let now = time::Instant::now();
        let withheld = {
            let stage = self.verified_stage();
            checks.withheld(level, |shard| stage.waiting_for(shard), now)
        };
        if let Some((shard, level_at, leaf_version)) = withheld {
            self.record_leaf_check("withheld");
            return Err(NetworkStateSyncError::UnverifiedHistory {
                details: format!(
                    "Peer {peer} claims shard {shard} is level at v{level_at}, but a verified leaf at v{leaf_version} \
                     has been ahead of it for {VERIFICATION_GRACE:?}"
                ),
                blamed: vec![*peer],
                diverged_from: None,
            });
        }
        if let Some((shard, version)) = checks.overdue(now) {
            self.record_leaf_check("unverified");
            let (rewound_to, _) = self.discard_staged(shard, progress).await?;
            return Err(NetworkStateSyncError::UnverifiedHistory {
                details: format!(
                    "Peer {peer} staged v{version} of shard {shard}, which no verified leaf has reached in \
                     {VERIFICATION_GRACE:?}. Discarded every version staged above v{rewound_to}"
                ),
                blamed: vec![*peer],
                diverged_from: None,
            });
        }
        Ok(())
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
    /// The group's checkpoint for the epoch before the current one was missing when the stream
    /// opened, and the work interval has passed. The group syncs checkpoints again and reopens.
    CheckpointsDue,
    /// The plan is being wound down.
    Cancelled,
}

/// How a round of following a shard group ended without failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoundEnd {
    /// The next round starts at once.
    Reopen,
    /// The next round waits the work interval first, for the given reason.
    Pause(&'static str),
    /// The plan is being wound down.
    Cancelled,
}

/// What a shard group's stream receives from the loop probing its tip.
struct GroupLeaves {
    /// The verified shard leaves of each probe.
    probed: mpsc::Receiver<Vec<VerifiedShardLeaf>>,
    /// Asks for a probe once the stream closes off a burst of versions.
    burst: Arc<tokio_sync::Notify>,
}

/// A cursor per shard resuming after the version recorded for it, in the order of `shards`. A shard
/// never synced resumes from version one. A shard recorded at the last representable version has no
/// version to resume from, and gets no cursor.
fn cursors_for(shards: &[Shard], progress: &SyncProgress) -> Vec<rpc::ShardCursor> {
    shards
        .iter()
        .filter_map(|&shard| {
            let start_state_version = match progress.last_state_version(shard) {
                Some(version) => {
                    let Some(next) = version.as_u64().checked_add(1) else {
                        warn!(target: LOG_TARGET, "⚠️ Shard {shard} is recorded at v{version}, the last representable state version, and is no longer synced. Resync this indexer from an empty data directory");
                        return None;
                    };
                    next
                },
                None => 1,
            };
            Some(rpc::ShardCursor {
                shard: shard.as_u32(),
                start_state_version,
            })
        })
        .collect()
}

/// Enforces the ordering a followed `sync_state` stream promises across the shards it carries: a
/// shard's versions follow one another from its cursor, and a version split across chunks is
/// delivered whole before anything else.
///
/// The consumer relies on both. Its buffers hold one `(shard, state version)` at a time, so a shard
/// interleaved mid-version would mix two shards' updates into one staged version; and each staged
/// version extends the accumulator of the one before it. Every committed state version of a shard
/// holds at least one transition, and the stream carries every transition as a value or a hash, so
/// a marker claims no version beyond the last one delivered. A completion marker closes off what
/// was streamed so far for a shard and a followed stream carries more for it after, so a marker
/// does not end a shard.
///
/// A version split across chunks is also held to [`MAX_BUFFERED_VERSION_BYTES`], since it is
/// buffered whole until the chunk that flushes it.
struct StreamOrder {
    /// Highest version delivered per requested shard, seeded from the cursor so a responder cannot
    /// replay versions the caller already holds.
    delivered_versions: HashMap<Shard, StateVersion>,
    /// Set while a version is split across chunks, with the encoded bytes buffered for it, until the
    /// chunk that flushes it.
    pending_chunk: Option<(Shard, StateVersion, usize)>,
    /// The shards delivered a version since their last marker.
    bursting: HashSet<Shard>,
}

impl StreamOrder {
    fn new(cursors: &[rpc::ShardCursor]) -> Self {
        Self {
            delivered_versions: cursors
                .iter()
                .map(|c| {
                    (
                        Shard::from(c.shard),
                        StateVersion::new(c.start_state_version.saturating_sub(1)),
                    )
                })
                .collect(),
            pending_chunk: None,
            bursting: HashSet::new(),
        }
    }

    fn accept_batch(
        &mut self,
        shard: Shard,
        state_version: StateVersion,
        has_more: bool,
        num_updates: usize,
        encoded_bytes: usize,
    ) -> Result<(), String> {
        let Some(delivered_version) = self.delivered_versions.get(&shard).copied() else {
            return Err(format!("Received batch for unrequested shard {shard}"));
        };
        // A responder splits a version into chunks of its updates, so every chunk carries one.
        if num_updates == 0 {
            return Err(format!("Received v{state_version} for shard {shard} with no updates"));
        }
        if state_version <= delivered_version {
            return Err(format!(
                "Received v{state_version} for shard {shard}, which is not ahead of the delivered v{delivered_version}"
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
                if delivered_version.as_u64().checked_add(1) != Some(state_version.as_u64()) {
                    return Err(format!(
                        "Received v{state_version} for shard {shard}, skipping the versions after the delivered \
                         v{delivered_version}"
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
            self.delivered_versions.insert(shard, state_version);
            self.bursting.insert(shard);
        }
        Ok(())
    }

    /// Accepts a completion marker, returning the version the shard is now level with the committee
    /// at - the last version delivered for it - and whether any was delivered since its last marker.
    fn accept_marker(&mut self, shard: Shard, synced_to: StateVersion) -> Result<(StateVersion, bool), String> {
        let Some(delivered_version) = self.delivered_versions.get(&shard).copied() else {
            return Err(format!("Received completion marker for unrequested shard {shard}"));
        };
        if let Some((pending_shard, pending_version, _)) = self.pending_chunk {
            return Err(format!(
                "Received completion marker for shard {shard} while v{pending_version} of shard {pending_shard} is \
                 still incomplete"
            ));
        }
        if synced_to > delivered_version {
            return Err(format!(
                "Received completion marker for shard {shard} at v{synced_to}, past the last version streamed, \
                 v{delivered_version}"
            ));
        }
        Ok((delivered_version, self.bursting.remove(&shard)))
    }
}

/// What a stream has staged above every verified leaf of its shards, and how long a verified leaf has been ahead of a
/// shard the stream claims is level. Both bound what a stream peer can do that no leaf has checked.
#[derive(Debug, Default)]
struct StreamChecks {
    /// Per shard, each version staged above every verified leaf: its version, encoded bytes and when it was staged,
    /// oldest first.
    uncovered: HashMap<Shard, VecDeque<(StateVersion, usize, time::Instant)>>,
    uncovered_bytes: usize,
    uncovered_versions: usize,
    /// When a verified leaf was first seen ahead of each shard the stream claims is level.
    behind_since: HashMap<Shard, time::Instant>,
}

impl StreamChecks {
    /// Records `version` of `shard`, staged at `now` with `bytes` encoded and above every verified leaf of the shard.
    fn stage(&mut self, shard: Shard, version: StateVersion, bytes: usize, now: time::Instant) {
        self.uncovered
            .entry(shard)
            .or_default()
            .push_back((version, bytes, now));
        self.uncovered_bytes = self.uncovered_bytes.saturating_add(bytes);
        self.uncovered_versions += 1;
    }

    /// Records a verified leaf of `shard` at `leaf_version`, which covers every version staged up to it.
    fn cover(&mut self, shard: Shard, leaf_version: StateVersion) {
        let Some(uncovered) = self.uncovered.get_mut(&shard) else {
            return;
        };
        while let Some(&(version, bytes, _)) = uncovered.front() &&
            version <= leaf_version
        {
            uncovered.pop_front();
            self.uncovered_bytes -= bytes;
            self.uncovered_versions -= 1;
        }
        if uncovered.is_empty() {
            self.uncovered.remove(&shard);
        }
    }

    /// Whether the stream has staged as much above every verified leaf as it may before a leaf covers some of it.
    fn is_full(&self) -> bool {
        self.uncovered_bytes >= MAX_UNCOVERED_BYTES || self.uncovered_versions >= MAX_UNCOVERED_VERSIONS
    }

    /// The oldest version no verified leaf has reached in [`VERIFICATION_GRACE`] since it was staged.
    fn overdue(&self, now: time::Instant) -> Option<(Shard, StateVersion)> {
        self.uncovered
            .iter()
            .filter_map(|(shard, uncovered)| {
                uncovered
                    .front()
                    .map(|&(version, _, staged_at)| (*shard, version, staged_at))
            })
            .filter(|&(_, _, staged_at)| now.duration_since(staged_at) > VERIFICATION_GRACE)
            .min_by_key(|&(_, _, staged_at)| staged_at)
            .map(|(shard, version, _)| (shard, version))
    }

    /// A shard of `level`, each level at the version given, that a verified leaf has been ahead of for longer than
    /// [`VERIFICATION_GRACE`], with its level version and the leaf's. `waiting_for` names the version of the leaf a
    /// shard is waiting for the stream to reach.
    fn withheld(
        &mut self,
        level: &HashMap<Shard, StateVersion>,
        waiting_for: impl Fn(Shard) -> Option<StateVersion>,
        now: time::Instant,
    ) -> Option<(Shard, StateVersion, StateVersion)> {
        self.behind_since.retain(|shard, _| level.contains_key(shard));
        let mut withheld = None;
        for (&shard, &level_at) in level {
            match waiting_for(shard) {
                Some(leaf_version) => {
                    let since = *self.behind_since.entry(shard).or_insert(now);
                    if now.duration_since(since) > VERIFICATION_GRACE {
                        withheld = Some((shard, level_at, leaf_version));
                    }
                },
                None => {
                    self.behind_since.remove(&shard);
                },
            }
        }
        withheld
    }
}

/// The peers whose history of a shard after a verified version mismatched a verified leaf or could not be
/// accumulated, by the shard and that version. Once every member of a committee has diverged from the same version,
/// the fault is this indexer's own: its recomputation of the accumulator disagrees with the committee's.
#[derive(Debug, Default)]
struct MismatchBreaker {
    mismatched: HashMap<(Shard, StateVersion), HashSet<PeerAddress>>,
}

impl MismatchBreaker {
    /// Records that `peers` diverged after `from` of `shard`, returning whether every one of the committee's
    /// `committee_size` members has.
    fn record(&mut self, shard: Shard, from: StateVersion, peers: &[PeerAddress], committee_size: usize) -> bool {
        let mismatched = self.mismatched.entry((shard, from)).or_default();
        mismatched.extend(peers.iter().copied());
        mismatched.len() >= committee_size
    }
}

/// A version staged with the verdict on a leaf that was waiting for it.
struct StagedWithVerdict {
    verdict: Option<Verdict<PeerAddress>>,
    accumulator: StagedAccumulator,
    batch: Vec<u8>,
}

impl StagedWithVerdict {
    fn into_parts(self) -> (Option<Verdict<PeerAddress>>, StagedVersion) {
        (self.verdict, StagedVersion {
            accumulator: self.accumulator,
            batch: self.batch,
        })
    }
}

fn decode_peer(bytes: &[u8]) -> Result<PeerAddress, NetworkStateSyncError> {
    libp2p::PeerId::from_bytes(bytes)
        .map(PeerAddress::from)
        .map_err(|e| NetworkStateSyncError::InvariantError {
            details: format!("a staged version records an invalid peer id: {e}"),
        })
}

fn display_peers(peers: &[PeerAddress]) -> String {
    peers.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

/// Adds `amount` to the running total persisted under `key`.
fn add_to_total(tx: &mut SqliteStoreWriteTransaction<'_>, key: Key, amount: Amount) -> Result<(), StorageError> {
    use crate::store::IndexerStoreWriteTransaction;

    let total = tx
        .key_value_get_value::<_, Amount>(&key)
        .optional()?
        .unwrap_or_else(Amount::zero);
    let new_total = total
        .checked_add(amount)
        .ok_or_else(|| StorageError::DataInconsistency {
            details: format!(
                "Adding {amount} to the {} total of {total} overflows it",
                key.as_key_str()
            ),
        })?;
    tx.key_value_set(key, new_total)
}

/// The exhaust burn `checkpoint` reports for its shard group and epoch.
///
/// Every Tari amount is a `u64` of microtari and the whole supply fits in one, so no shard group can
/// burn more in an epoch. A checkpoint claiming more is rejected: bounding each one keeps the running
/// total, summed over every epoch and shard group, from approaching `Amount::MAX`.
fn checkpoint_exhaust_burn(checkpoint: &EpochCheckpoint) -> Result<Amount, NetworkStateSyncError> {
    let burn = checkpoint.header().accumulated_data().total_exhaust_burn;
    u64::try_from(burn)
        .map(Amount::from)
        .map_err(|_| NetworkStateSyncError::InvalidCheckpoint {
            details: format!(
                "Checkpoint for epoch {} claims an exhaust burn of {burn}, more than the total supply",
                checkpoint.epoch()
            ),
        })
}

/// Requests `shard_group`'s checkpoints from `from_epoch` on from `session`'s committee member.
async fn fetch_checkpoints(
    session: &mut ValidatorRpcSession,
    shard_group: ShardGroup,
    from_epoch: Epoch,
) -> Result<Vec<EpochCheckpoint>, NetworkStateSyncError> {
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
            EpochCheckpoint::try_from(cp).map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                details: format!("Failed to convert checkpoint for shard group {}: {}", shard_group, e),
            })
        })
        .collect()
}

/// How far [`verify_checkpoint`] could check a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckpointCheck {
    /// Its committee's quorum signed it.
    QuorumSigned,
    /// Its epoch is before this indexer's records, so it was checked for internal consistency only.
    FormOnly,
}

/// Verifies `checkpoint` against `epoch_manager`'s record of its epoch, before it is stored and served.
///
/// In an epoch the epoch manager has a record of, the checkpoint must be of one of that epoch's shard groups, and its
/// quorum certificate is checked against that shard group's committee. `shard_group` is the checkpoint's own, which
/// can differ from the shard group being synced because shard groups change with the number of committees. Only a
/// checkpoint of an epoch before the epoch manager's records is checked for form alone. `prev_epoch` is always
/// recorded.
///
/// The next epoch's validator set the checkpoint names, which the committee ratified, must be the one this indexer
/// assigned: an indexer that disagrees would serve committees the network does not run.
async fn verify_checkpoint<TEpochManager: EpochManagerReader>(
    network: Network,
    epoch_manager: &TEpochManager,
    checkpoint: &EpochCheckpoint,
    shard_group: ShardGroup,
    prev_epoch: Epoch,
) -> Result<CheckpointCheck, NetworkStateSyncError> {
    let epoch = checkpoint.epoch();
    let recorded = epoch == prev_epoch || epoch_manager.get_epoch_hash(epoch).await.optional()?.is_some();
    if !recorded {
        debug!(target: LOG_TARGET, "🌍️ {epoch} is before this indexer's records: checking its checkpoint for form only");
        checkpoint
            .validate_well_formed()
            .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                details: format!("Checkpoint for {epoch}, shard group {shard_group} is not well formed: {e}"),
            })?;
        return Ok(CheckpointCheck::FormOnly);
    }
    let Some(committee) = epoch_manager
        .get_committee_by_shard_group(epoch, shard_group)
        .await
        .optional()?
    else {
        return Err(NetworkStateSyncError::InvalidCheckpoint {
            details: format!(
                "Checkpoint for {epoch} names shard group {shard_group}, which is not one of that epoch's shard \
                 groups as this indexer assigned them. If every committee member is refused this way, this indexer's \
                 committee assignment for {epoch} differs from the network's; check its epoch oracle and committee \
                 configuration."
            ),
        });
    };
    checkpoint
        .validate(network, epoch, committee.quorum_threshold(), &*committee)
        .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
            details: format!("Checkpoint for {epoch}, shard group {shard_group} is not valid: {e}"),
        })?;

    let Some(committed) = checkpoint.next_epoch_validator_set_hash() else {
        return Ok(CheckpointCheck::QuorumSigned);
    };
    let next_epoch = epoch + Epoch(1);
    let local = epoch_manager.get_validator_set_hash(next_epoch).await?;
    check_validator_set(next_epoch, local, committed)?;
    Ok(CheckpointCheck::QuorumSigned)
}

fn check_validator_set(
    epoch: Epoch,
    local: ValidatorSetHash,
    committed: ValidatorSetHash,
) -> Result<(), NetworkStateSyncError> {
    if local != committed {
        return Err(NetworkStateSyncError::ValidatorSetDiverged {
            epoch,
            local,
            committed,
        });
    }
    Ok(())
}

/// Stores `checkpoint` of `shard_group` with its verified `leaves`, adding `xtr_exhausted` to the accumulated exhaust
/// burn the first time it is stored, and `progress` when given.
fn write_checkpoint(
    tx: &mut SqliteStoreWriteTransaction<'_>,
    checkpoint: &EpochCheckpoint,
    shard_group: ShardGroup,
    xtr_exhausted: Amount,
    leaves: &[CheckpointShardLeaf],
    progress: Option<SyncProgress>,
) -> Result<(), StorageError> {
    if !tx.epoch_checkpoint_exists(shard_group, checkpoint.epoch())? {
        tx.insert_or_ignore_epoch_checkpoint(checkpoint)?;
        add_to_total(tx, Key::TariAccumulatedExhaustBurn, xtr_exhausted)?;
    }
    tx.checkpoint_shard_leaves_insert(leaves)?;
    if let Some(progress) = progress {
        tx.key_value_set(Key::SyncProgress, progress)?;
    }
    Ok(())
}

/// Whether a checkpoint sync from `from_epoch` backfills epochs before `prev_epoch`, whose checkpoint is then synced
/// first.
fn is_backfill(from_epoch: Epoch, prev_epoch: Epoch) -> bool {
    from_epoch.as_u64() + 1 < prev_epoch.as_u64()
}

/// Keeps the checkpoints a sync from `from_epoch` can use: those from `from_epoch` up to `prev_epoch`.
/// Later ones are for an epoch that has not ended for this indexer, and earlier ones were already
/// synced.
fn retain_checkpoints_in_sync_range(checkpoints: &mut Vec<EpochCheckpoint>, from_epoch: Epoch, prev_epoch: Epoch) {
    checkpoints.retain(|checkpoint| (from_epoch..=prev_epoch).contains(&checkpoint.epoch()));
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

/// One staged state version, decoded and sorted into the buffers it is committed to the indexed tables from.
#[derive(Default)]
struct VersionBuf {
    invalidations: Vec<SubstateCacheInvalidation>,
    utxos: Vec<UtxoUpdateRecord>,
    transactions: Vec<(TransactionReceiptAddress, TransactionReceipt)>,
    validator_fee_pools: Vec<SubstateData>,
    template_catalogue: Vec<(TemplateAddress, PublishedTemplateMetadata)>,
    xtr_claimed: Amount,
    /// The UTXO versions this version burns: each is taken down as the next version comes up with no output.
    burnt_utxos: HashSet<(UtxoAddress, SubstateVersion)>,
}

/// What committing a [`VersionBuf`] wrote, for the caller to announce once the write transaction
/// has committed.
struct CommittedVersion {
    retired_cache_entries: usize,
    /// Events carried by the inserted receipts, before the event filters.
    num_receipt_events: usize,
    /// One per receipt the commit inserted.
    finalized: Vec<TransactionFinalizedEvent>,
    inserted_events: Vec<InsertedEvent>,
    /// Unspent outputs left out because their version is indexed already.
    skipped_utxos: Vec<UtxoAddress>,
}

/// What promoting a run of a shard's staged versions wrote, for the caller to announce once the write transaction has
/// committed.
#[derive(Default)]
struct PromotedVersions {
    retired_cache_entries: usize,
    num_receipt_events: usize,
    finalized: Vec<TransactionFinalizedEvent>,
    inserted_events: Vec<InsertedEvent>,
    skipped_utxos: Vec<(StateVersion, UtxoAddress)>,
}

impl PromotedVersions {
    fn add(&mut self, state_version: StateVersion, committed: CommittedVersion) {
        self.retired_cache_entries += committed.retired_cache_entries;
        self.num_receipt_events += committed.num_receipt_events;
        self.finalized.extend(committed.finalized);
        self.inserted_events.extend(committed.inserted_events);
        self.skipped_utxos.extend(
            committed
                .skipped_utxos
                .into_iter()
                .map(|address| (state_version, address)),
        );
    }
}

/// Promotes the versions of `shard` staged above `from`, up to `through`, to the indexed tables in order, as many as
/// [`MAX_PROMOTED_VERSIONS`] and [`MAX_PROMOTED_BYTES`] allow, and deletes them from the stage. The last one promoted
/// is recorded as verified in `progress`, which is written with them.
///
/// A leaf that matches the accumulator at `through` authenticates every version below it too, so each promoted version
/// is verified whether or not the run reaches `through`.
fn promote_staged(
    tx: &mut SqliteStoreWriteTransaction<'_>,
    shard: Shard,
    from: StateVersion,
    through: StateVersion,
    mut progress: SyncProgress,
    event_filters: &[EventFilter],
    watched_templates: &HashSet<TemplateAddress>,
) -> Result<(VerifiedShardVersion, PromotedVersions), StorageError> {
    let mut promoted = PromotedVersions::default();
    let mut last = None;
    let mut num_versions = 0;
    let mut num_bytes = 0usize;
    let mut version = from;
    while version < through && num_versions < MAX_PROMOTED_VERSIONS && num_bytes < MAX_PROMOTED_BYTES {
        version = StateVersion::new(version.as_u64() + 1);
        let staged = tx
            .staged_version_get(shard, version)?
            .ok_or_else(|| StorageError::DataInconsistency {
                details: format!(
                    "v{version} of shard {shard} is verified, but it is not staged and was not promoted before"
                ),
            })?;
        let epoch = staged.accumulator.epoch;
        let committed = VersionBuf::decode_staged(shard, version, &staged.batch)?.commit(
            tx,
            shard,
            version,
            epoch,
            event_filters,
            watched_templates,
        )?;
        promoted.add(version, committed);
        num_versions += 1;
        num_bytes = num_bytes.saturating_add(staged.batch.len());
        last = Some(staged.accumulator);
    }
    let last = last.ok_or_else(|| StorageError::DataInconsistency {
        details: format!("promoting shard {shard} from v{from} through v{through} promotes nothing"),
    })?;
    tx.staged_versions_delete_through(shard, last.state_version)?;
    tx.checkpoint_shard_leaves_delete_through(shard, last.state_version)?;
    let verified = VerifiedShardVersion {
        state_version: last.state_version,
        epoch: last.epoch,
        transition_acc: last.transition_acc,
    };
    progress.record_verified(shard, verified);
    tx.key_value_set(Key::SyncProgress, progress)?;
    Ok((verified, promoted))
}

impl VersionBuf {
    /// Decodes a staged version from its encoded `SubstateBatch` chunks.
    fn decode_staged(shard: Shard, state_version: StateVersion, mut batch: &[u8]) -> Result<Self, StorageError> {
        let invalid = |details: String| StorageError::DataInconsistency {
            details: format!("staged v{state_version} of shard {shard} does not decode: {details}"),
        };
        let value_filters = stream_value_filters();
        let mut buf = Self::default();
        while !batch.is_empty() {
            let chunk = rpc::SubstateBatch::decode_length_delimited(&mut batch).map_err(|e| invalid(e.to_string()))?;
            for update in chunk.updates {
                let update = SubstateUpdateProof::try_from(update).map_err(|e| invalid(e.to_string()))?;
                buf.extend_from_substate_update(shard, state_version, update, value_filters);
            }
        }
        buf.mark_burns();
        Ok(buf)
    }

    /// Marks each spend of a UTXO version whose next version this version brings up with no output as a burn. A burn
    /// takes a version down and brings the next one up in the same transaction, and a UTXO's versions share its shard,
    /// so both are in this version.
    fn mark_burns(&mut self) {
        if self.burnt_utxos.is_empty() {
            return;
        }
        for update in &mut self.utxos {
            if let UtxoUpdateRecord::Spent(spent) = update &&
                self.burnt_utxos.contains(&(spent.address.clone(), spent.version))
            {
                *update = UtxoUpdateRecord::Burnt(spent.clone());
            }
        }
    }

    /// Writes the version to the indexed tables and folds what it inserted into the running totals.
    ///
    /// Every version committed is verified, and a receipt or template is created once on the network, so one already
    /// indexed is a fault in this indexer's store or its verification, and fails the commit.
    fn commit(
        self,
        tx: &mut SqliteStoreWriteTransaction<'_>,
        shard: Shard,
        state_version: StateVersion,
        epoch: Epoch,
        event_filters: &[EventFilter],
        watched_templates: &HashSet<TemplateAddress>,
    ) -> Result<CommittedVersion, StorageError> {
        // Must commit with the watermark below: the substate cache serves an entry on the argument
        // that it holds every transition up to that watermark, which a reader seeing one of the two
        // without the other would break.
        let retired_cache_entries = tx.substate_cache_invalidate(self.invalidations, state_version)?;
        debug!(target: LOG_TARGET, "✅ Committing {} UTXOs for shard {shard} (epoch: {epoch})", self.utxos.len());
        let skipped_utxos = tx.batch_insert_utxo_updates(epoch, self.utxos)?;
        for substate_data in self.validator_fee_pools {
            tx.upsert_substate(&substate_data)?;
        }
        debug!(target: LOG_TARGET, "✅ Committing {} transactions for shard {shard} (epoch: {epoch})", self.transactions.len());
        let inserted = tx.batch_insert_transaction_receipts(self.transactions, event_filters)?;
        if let Some(address) = inserted.skipped.first() {
            return Err(StorageError::DataInconsistency {
                details: format!(
                    "verified v{state_version} of shard {shard} creates receipt {address}, which is already indexed"
                ),
            });
        }
        if !watched_templates.is_empty() {
            process_watched_substate_events(tx, &inserted.events, watched_templates)?;
        }

        if !self.template_catalogue.is_empty() {
            debug!(target: LOG_TARGET, "✅ Inserting {} template catalogue entries for shard {shard} (epoch: {epoch})", self.template_catalogue.len());
            for (template_addr, metadata) in self.template_catalogue {
                if !tx.insert_or_ignore_template_catalogue(&template_addr, &metadata)? {
                    return Err(StorageError::DataInconsistency {
                        details: format!(
                            "verified v{state_version} of shard {shard} publishes template {template_addr}, which is \
                             already catalogued"
                        ),
                    });
                }
            }
        }

        // Each receipt's realized-share pair: what the payer spent and the burn taken out of it, so
        // `burn / paid` recovers the share independent of the header-sourced burn total.
        let mut xtr_fees = Amount::zero();
        let mut xtr_receipt_burn = Amount::zero();
        for receipt in &inserted.receipts {
            xtr_fees += Amount::from(receipt.fee_receipt.total_fees_paid());
            xtr_receipt_burn += Amount::from(receipt.fee_receipt.exhaust_burn());
        }

        add_to_total(tx, Key::TariAccumulatedClaimed, self.xtr_claimed)?;
        add_to_total(tx, Key::TariAccumulatedFees, xtr_fees)?;
        add_to_total(tx, Key::TariAccumulatedReceiptExhaustBurn, xtr_receipt_burn)?;

        Ok(CommittedVersion {
            retired_cache_entries,
            num_receipt_events: inserted.receipts.iter().map(|receipt| receipt.num_events).sum(),
            finalized: inserted
                .receipts
                .into_iter()
                .map(|receipt| TransactionFinalizedEvent {
                    transaction_id: receipt.transaction_id,
                    outcome: receipt.outcome,
                })
                .collect(),
            inserted_events: inserted.events,
            skipped_utxos,
        })
    }

    /// Sorts one streamed transition into the buffers the commit is assembled from.
    ///
    /// A transition that retires anything cached reaches `invalidations`, which for a substate's first
    /// creation is the record that it did not exist. Only those whose substate `value_filters` selects
    /// carry a value; the rest arrive as an id and a version under `ALL_HASHES` and must reach nothing
    /// else - indexing one, counting it in the economic totals or emitting an event for it would all be
    /// reading a value that was never sent.
    fn extend_from_substate_update(
        &mut self,
        shard: Shard,
        state_version: StateVersion,
        update: SubstateUpdateProof,
        value_filters: SubstateValueFilterFlags,
    ) {
        self.invalidations.extend(match &update {
            SubstateUpdateProof::Create(create) => {
                SubstateCacheInvalidation::created(create.substate.substate_id(), create.substate.version)
            },
            SubstateUpdateProof::Destroy(destroy) => Some(SubstateCacheInvalidation::destroyed(
                destroy.substate_id.clone(),
                destroy.version,
            )),
        });

        if !value_filters.contains_substate(update.substate_id()) {
            return;
        }

        match &update {
            SubstateUpdateProof::Create(create) => {
                if create.substate.substate_id().is_template() {
                    if let Some(metadata) = &create.substate.template_metadata &&
                        let Some(template_addr) = create.substate.substate_id().as_template()
                    {
                        self.template_catalogue
                            .push((template_addr.as_template_address(), metadata.clone()));
                    }
                    return;
                }
                match create.substate.value().value() {
                    Some(SubstateValue::Utxo(utxo)) => {
                        if let Some(address) = create.substate.substate_id().as_utxo_address() {
                            let is_frozen = utxo.is_frozen();
                            match utxo.output {
                                Some(ref output) => {
                                    self.utxos.push(UtxoUpdateRecord::Unspent(Box::new(UtxoUnspent {
                                        address,
                                        version: update.version(),
                                        shard,
                                        state_version,
                                        utxo_output: output.clone(),
                                        is_frozen,
                                    })));
                                },
                                None => {
                                    if let Some(burnt) = update.version().previous() {
                                        self.burnt_utxos.insert((address, burnt));
                                    }
                                },
                            }
                        } else {
                            warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received UTXO substate with invalid address: {}", create.substate.substate_id());
                        };
                    },
                    Some(SubstateValue::TransactionReceipt(receipt)) => {
                        if let Some(address) = update.substate_id().as_transaction_receipt_address() {
                            self.transactions.push((address, receipt.clone()));
                        } else {
                            warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received Transaction Receipt substate with invalid address: {}", create.substate.substate_id());
                        }
                    },
                    Some(SubstateValue::ValidatorFeePool(_)) => {
                        self.validator_fee_pools.push(SubstateData {
                            substate_id: create.substate.substate_id().clone(),
                            version: create.substate.version,
                            value: create.substate.value().clone(),
                            template_metadata: None,
                        });
                    },
                    Some(SubstateValue::ClaimedOutputTombstone(claim)) => {
                        self.xtr_claimed += Amount::from(claim.value);
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
                    self.utxos.push(UtxoUpdateRecord::Spent(UtxoSpent {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod running_totals {
        use super::*;
        use crate::store::{IndexerStore, IndexerStoreWriteTransaction};

        async fn store_with_total(key: Key, total: Amount) -> (tempfile::TempDir, SqliteIndexerStore) {
            let dir = tempfile::tempdir().unwrap();
            let store = SqliteIndexerStore::try_create(dir.path().join("indexer.db")).unwrap();
            store
                .with_write_tx(move |tx| tx.key_value_set(key, total))
                .await
                .unwrap();
            (dir, store)
        }

        async fn total(store: &SqliteIndexerStore, key: Key) -> Amount {
            store
                .with_read_tx(move |tx| tx.key_value_get_value::<_, Amount>(key))
                .await
                .unwrap()
        }

        #[tokio::test]
        async fn an_addition_that_overflows_the_total_is_refused() {
            let (_dir, store) = store_with_total(Key::TariAccumulatedExhaustBurn, Amount::MAX).await;

            let result = store
                .with_write_tx(|tx| add_to_total(tx, Key::TariAccumulatedExhaustBurn, Amount::from(1u64)))
                .await;

            assert!(
                matches!(result, Err(StorageError::DataInconsistency { .. })),
                "{result:?}"
            );
            assert_eq!(total(&store, Key::TariAccumulatedExhaustBurn).await, Amount::MAX);
        }

        #[tokio::test]
        async fn an_addition_is_folded_into_the_total() {
            let (_dir, store) = store_with_total(Key::TariAccumulatedFees, Amount::from(40u64)).await;

            store
                .with_write_tx(|tx| add_to_total(tx, Key::TariAccumulatedFees, Amount::from(2u64)))
                .await
                .unwrap();

            assert_eq!(total(&store, Key::TariAccumulatedFees).await, Amount::from(42u64));
        }
    }

    mod burns {
        use tari_engine_types::{Utxo, UtxoOutput};
        use tari_ootle_storage::consensus_models::{SubstateCreate, SubstateDestroy, SubstateValueOrHash};

        use super::*;
        use crate::storage_sqlite::unspent_at;

        const SHARD: Shard = Shard::from_u32(1);

        fn output() -> (UtxoAddress, UtxoOutput) {
            let UtxoUpdateRecord::Unspent(unspent) = unspent_at(1, 1) else {
                unreachable!()
            };
            (unspent.address, unspent.utxo_output)
        }

        fn down(address: &UtxoAddress, version: u64) -> SubstateUpdateProof {
            SubstateUpdateProof::Destroy(SubstateDestroy {
                substate_id: SubstateId::Utxo(address.clone()),
                version: SubstateVersion::new(version),
            })
        }

        fn up(address: &UtxoAddress, version: u64, output: Option<UtxoOutput>) -> SubstateUpdateProof {
            SubstateUpdateProof::from(SubstateCreate {
                substate: SubstateData {
                    substate_id: SubstateId::Utxo(address.clone()),
                    version: SubstateVersion::new(version),
                    value: SubstateValueOrHash::Value(Box::new(SubstateValue::Utxo(Utxo {
                        output,
                        is_frozen: false,
                    }))),
                    template_metadata: None,
                },
            })
        }

        fn sorted(updates: Vec<SubstateUpdateProof>) -> VersionBuf {
            let mut buf = VersionBuf::default();
            for update in updates {
                buf.extend_from_substate_update(SHARD, StateVersion::new(5), update, stream_value_filters());
            }
            buf.mark_burns();
            buf
        }

        /// A burn takes the output's version down and brings the next one up with no output, in either order.
        #[test]
        fn a_burn_marks_the_version_it_takes_down_burnt() {
            let (address, _) = output();
            for updates in [vec![down(&address, 0), up(&address, 1, None)], vec![
                up(&address, 1, None),
                down(&address, 0),
            ]] {
                let buf = sorted(updates);

                assert_eq!(buf.utxos.len(), 1);
                assert!(
                    matches!(&buf.utxos[0], UtxoUpdateRecord::Burnt(burnt)
                        if burnt.address == address && burnt.version == SubstateVersion::ZERO),
                    "{:?}",
                    buf.utxos[0]
                );
            }
        }

        /// Only the version taken down by the burn is burnt when an unfreeze and a burn share a state version.
        #[test]
        fn a_burn_marks_only_the_version_it_takes_down() {
            let (address, output) = output();

            let buf = sorted(vec![
                down(&address, 0),
                up(&address, 1, Some(output)),
                down(&address, 1),
                up(&address, 2, None),
            ]);

            let downs = buf
                .utxos
                .iter()
                .filter_map(|update| match update {
                    UtxoUpdateRecord::Spent(spent) => Some((false, spent.version)),
                    UtxoUpdateRecord::Burnt(burnt) => Some((true, burnt.version)),
                    UtxoUpdateRecord::Unspent(_) => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(downs, [(false, SubstateVersion::ZERO), (true, SubstateVersion::new(1))]);
        }

        /// A freeze takes the output's version down and brings the next one up with the output.
        #[test]
        fn a_re_versioning_with_the_output_is_a_spend() {
            let (address, output) = output();

            let buf = sorted(vec![down(&address, 0), up(&address, 1, Some(output))]);

            assert_eq!(buf.utxos.len(), 2);
            assert!(matches!(&buf.utxos[0], UtxoUpdateRecord::Spent(spent) if spent.version == SubstateVersion::ZERO));
            assert!(
                matches!(&buf.utxos[1], UtxoUpdateRecord::Unspent(unspent) if unspent.version == SubstateVersion::new(1))
            );
        }
    }

    mod version_commit {
        use tari_engine_types::{fees::FeeReceiptBuilder, transaction_receipt::FinalizeOutcome};
        use tari_ootle_storage::consensus_models::{SubstateCreate, SubstateValueOrHash};
        use tari_ootle_transaction::TransactionId;
        use tari_state_tree::TransitionAccumulator;
        use tari_template_lib_types::{Hash32, crypto::RistrettoPublicKeyBytes};

        use super::*;
        use crate::store::IndexerStore;

        const SHARD: Shard = Shard::from_u32(1);

        fn v(n: u64) -> StateVersion {
            StateVersion::new(n)
        }

        fn transaction_id(n: u64) -> TransactionId {
            let mut id = [0; 32];
            id[..8].copy_from_slice(&n.to_le_bytes());
            TransactionId::new(id)
        }

        fn receipt_address(n: u64) -> TransactionReceiptAddress {
            transaction_id(n).into_receipt_address()
        }

        fn receipt(outcome: FinalizeOutcome, total_fees_paid: u64) -> TransactionReceipt {
            TransactionReceipt {
                outcome,
                diff_summary: Default::default(),
                fee_withdrawals: [].into(),
                events: [].into(),
                fee_receipt: FeeReceiptBuilder::default()
                    .with_total_fees_paid(total_fees_paid)
                    .build(),
                epoch: Epoch(1),
                intent_commitment: Default::default(),
            }
        }

        fn template_metadata(name: &str) -> PublishedTemplateMetadata {
            PublishedTemplateMetadata {
                template_name: name.to_string(),
                author_public_key: RistrettoPublicKeyBytes::default(),
                binary_hash: Hash32::default(),
                at_epoch: 1,
                metadata_hash: None,
            }
        }

        async fn store() -> (tempfile::TempDir, SqliteIndexerStore) {
            let dir = tempfile::tempdir().unwrap();
            let store = SqliteIndexerStore::try_create(dir.path().join("indexer.db")).unwrap();
            (dir, store)
        }

        async fn commit(
            store: &SqliteIndexerStore,
            version: u64,
            buf: VersionBuf,
        ) -> Result<CommittedVersion, StorageError> {
            store
                .with_write_tx(move |tx| buf.commit(tx, SHARD, v(version), Epoch(1), &[], &HashSet::new()))
                .await
        }

        async fn total_fees(store: &SqliteIndexerStore) -> Amount {
            store
                .with_read_tx(|tx| tx.key_value_get_value::<_, Amount>(Key::TariAccumulatedFees))
                .await
                .optional()
                .unwrap()
                .unwrap_or_else(Amount::zero)
        }

        /// A receipt is created once on the network and every committed version is verified, so a second receipt at an
        /// indexed address is a fault of this indexer's, and nothing of its version is written.
        #[tokio::test]
        async fn a_receipt_already_indexed_fails_the_commit() {
            let (_dir, store) = store().await;
            commit(&store, 1, VersionBuf {
                transactions: vec![(receipt_address(1), receipt(FinalizeOutcome::FeeIntentCommit, 100))],
                ..Default::default()
            })
            .await
            .unwrap();

            let result = commit(&store, 2, VersionBuf {
                transactions: vec![
                    (receipt_address(2), receipt(FinalizeOutcome::Commit, 5)),
                    (receipt_address(1), receipt(FinalizeOutcome::Commit, 999)),
                ],
                ..Default::default()
            })
            .await;

            assert!(
                matches!(result, Err(StorageError::DataInconsistency { .. })),
                "{:?}",
                result.err()
            );
            assert_eq!(total_fees(&store).await, Amount::from(100u64));
            let second = store
                .with_read_tx(|tx| tx.get_transaction_receipt(&receipt_address(2)))
                .await
                .optional()
                .unwrap();
            assert!(second.is_none());
        }

        #[tokio::test]
        async fn a_catalogued_template_fails_the_commit() {
            let (_dir, store) = store().await;
            let address = TemplateAddress::from_array([7; 32]);

            commit(&store, 1, VersionBuf {
                template_catalogue: vec![(address, template_metadata("original"))],
                ..Default::default()
            })
            .await
            .unwrap();
            let result = commit(&store, 2, VersionBuf {
                template_catalogue: vec![(address, template_metadata("rewritten"))],
                ..Default::default()
            })
            .await;

            assert!(
                matches!(result, Err(StorageError::DataInconsistency { .. })),
                "{:?}",
                result.err()
            );
            let entry = store
                .with_read_tx(move |tx| tx.get_template_catalogue_entry(&address))
                .await
                .unwrap();
            assert_eq!(entry.template_name, "original");
        }

        /// The encoded chunks of a version that creates the receipt of transaction `n`, paying `fees`, and that of
        /// transaction `n + 10_000`, paying nothing, each in a chunk of its own.
        fn encoded_receipt_version(version: u64, n: u64, fees: u64) -> Vec<u8> {
            let create = |n: u64, fees: u64| -> rpc::SubstateUpdate {
                SubstateUpdateProof::from(SubstateCreate {
                    substate: SubstateData {
                        substate_id: SubstateId::TransactionReceipt(receipt_address(n)),
                        version: tari_engine_types::SubstateVersion::ZERO,
                        value: SubstateValueOrHash::Value(Box::new(SubstateValue::TransactionReceipt(receipt(
                            FinalizeOutcome::Commit,
                            fees,
                        )))),
                        template_metadata: None,
                    },
                })
                .into()
            };
            let chunk = |updates, has_more| rpc::SubstateBatch {
                state_version: version,
                updates,
                has_more,
                epoch: Some(Epoch(1).into()),
                shard: SHARD.as_u32(),
            };
            let mut encoded = chunk(vec![create(n, fees)], true).encode_length_delimited_to_vec();
            encoded.extend(chunk(vec![create(n + 10_000, 0)], false).encode_length_delimited_to_vec());
            encoded
        }

        fn acc(n: u64) -> TransitionAccumulator {
            let mut acc = [0; 32];
            acc[..8].copy_from_slice(&n.to_le_bytes());
            TransitionAccumulator::new(acc)
        }

        /// Stages v1..=`num_versions` of [`SHARD`], each creating the receipt of transaction `n` with `n` in fees, as a
        /// stream would, and records the cursor at the last.
        async fn stage(store: &SqliteIndexerStore, num_versions: u64) -> SyncProgress {
            let mut progress = SyncProgress::default();
            progress.record_state_version(SHARD, v(num_versions), Epoch(1));
            let snapshot = progress.clone();
            store
                .with_write_tx(move |tx| {
                    for n in 1..=num_versions {
                        tx.staged_version_insert(SHARD, &StagedVersion {
                            accumulator: StagedAccumulator {
                                state_version: v(n),
                                epoch: Epoch(1),
                                transition_acc: acc(n),
                                streamed_by: vec![u8::try_from(n % 2).unwrap()],
                            },
                            batch: encoded_receipt_version(n, n, n),
                        })?;
                    }
                    tx.key_value_set(Key::SyncProgress, snapshot)
                })
                .await
                .unwrap();
            progress
        }

        async fn promote(
            store: &SqliteIndexerStore,
            progress: &SyncProgress,
            through: u64,
        ) -> Result<(VerifiedShardVersion, PromotedVersions), StorageError> {
            let progress = progress.clone();
            store
                .with_write_tx(move |tx| {
                    let from = progress.verified(SHARD).version;
                    promote_staged(tx, SHARD, from, v(through), progress, &[], &HashSet::new())
                })
                .await
        }

        async fn stored_progress(store: &SqliteIndexerStore) -> SyncProgress {
            store
                .with_read_tx(|tx| tx.key_value_get_value::<_, SyncProgress>(Key::SyncProgress))
                .await
                .unwrap()
        }

        async fn staged_versions(store: &SqliteIndexerStore) -> Vec<u64> {
            store
                .with_read_tx(|tx| tx.staged_accumulators_tail(SHARD, usize::MAX))
                .await
                .unwrap()
                .into_iter()
                .map(|staged| staged.state_version.as_u64())
                .collect()
        }

        #[tokio::test]
        async fn promotion_indexes_the_staged_versions_through_the_leaf_and_records_the_last_as_verified() {
            let (_dir, store) = store().await;
            let mut progress = stage(&store, 4).await;

            let (verified, promoted) = promote(&store, &progress, 3).await.unwrap();

            assert_eq!(verified, VerifiedShardVersion {
                state_version: v(3),
                epoch: Epoch(1),
                transition_acc: acc(3),
            });
            let finalized = promoted
                .finalized
                .iter()
                .map(|f| f.transaction_id)
                .collect::<HashSet<_>>();
            assert_eq!(finalized.len(), 6);
            assert!(finalized.contains(&transaction_id(3)));
            assert!(!finalized.contains(&transaction_id(4)));
            assert_eq!(total_fees(&store).await, Amount::from(1u64 + 2 + 3));
            assert_eq!(staged_versions(&store).await, [4]);
            let stored = stored_progress(&store).await;
            assert_eq!(stored.verified.get(&SHARD), Some(&verified));
            assert_eq!(stored.last_state_version(SHARD), Some(v(4)));

            progress.record_verified(SHARD, verified);
            promote(&store, &progress, 4).await.unwrap();
            assert!(staged_versions(&store).await.is_empty());
            assert_eq!(total_fees(&store).await, Amount::from(1u64 + 2 + 3 + 4));
        }

        #[tokio::test]
        async fn promotion_of_a_long_run_stops_at_the_transaction_cap_with_the_last_version_promoted_verified() {
            let (_dir, store) = store().await;
            let num_versions = MAX_PROMOTED_VERSIONS as u64 + 2;
            let progress = stage(&store, num_versions).await;

            let (verified, _) = promote(&store, &progress, num_versions).await.unwrap();

            assert_eq!(verified.state_version, v(MAX_PROMOTED_VERSIONS as u64));
            assert_eq!(staged_versions(&store).await, [num_versions - 1, num_versions]);
        }

        /// A version that is verified must be staged; a gap in the stage means the store lost it.
        #[tokio::test]
        async fn promotion_through_a_version_that_is_not_staged_fails() {
            let (_dir, store) = store().await;
            let progress = stage(&store, 2).await;

            let result = promote(&store, &progress, 3).await;

            assert!(
                matches!(result, Err(StorageError::DataInconsistency { .. })),
                "{:?}",
                result.err()
            );
            assert_eq!(staged_versions(&store).await, [1, 2]);
            assert_eq!(total_fees(&store).await, Amount::zero());
        }

        #[tokio::test]
        async fn discarding_above_the_verified_version_leaves_the_indexed_tables_and_totals_as_they_were() {
            let (_dir, store) = store().await;
            let mut progress = stage(&store, 4).await;
            let (verified, _) = promote(&store, &progress, 1).await.unwrap();
            progress.record_verified(SHARD, verified);

            progress.rewind_to_verified(SHARD);
            let snapshot = progress.clone();
            let peers = store
                .with_write_tx(move |tx| {
                    let peers = tx.staged_versions_discard_above(SHARD, v(1))?;
                    tx.key_value_set(Key::SyncProgress, snapshot)?;
                    Ok::<_, StorageError>(peers)
                })
                .await
                .unwrap();

            assert_eq!(
                peers.into_iter().collect::<HashSet<_>>(),
                HashSet::from([vec![0], vec![1]])
            );
            assert!(staged_versions(&store).await.is_empty());
            assert_eq!(total_fees(&store).await, Amount::from(1u64));
            assert_eq!(stored_progress(&store).await.last_state_version(SHARD), Some(v(1)));
            for n in 2..=4 {
                let receipt = store
                    .with_read_tx(move |tx| tx.get_transaction_receipt(&receipt_address(n)))
                    .await
                    .optional()
                    .unwrap();
                assert!(receipt.is_none(), "v{n}'s receipt is indexed");
            }
        }

        fn checkpoint_leaf(n: u64) -> CheckpointShardLeaf {
            CheckpointShardLeaf {
                shard: SHARD,
                state_version: v(n),
                transition_acc: acc(n),
            }
        }

        async fn checkpoint_leaves_above(store: &SqliteIndexerStore, n: u64, limit: usize) -> Vec<u64> {
            store
                .with_read_tx(move |tx| tx.checkpoint_shard_leaves_above(SHARD, v(n), limit))
                .await
                .unwrap()
                .into_iter()
                .map(|leaf| leaf.state_version.as_u64())
                .collect()
        }

        /// A catch-up checks its staged versions against every stored checkpoint leaf in turn, the next few above the
        /// verified version at a time, and promotion drops those it has passed.
        #[tokio::test]
        async fn stored_checkpoint_leaves_are_read_in_order_above_the_verified_version_and_dropped_once_promoted() {
            let (_dir, store) = store().await;
            let progress = stage(&store, 6).await;
            store
                .with_write_tx(|tx| {
                    tx.checkpoint_shard_leaves_insert(&[2, 4, 6, 9].map(checkpoint_leaf))?;
                    tx.checkpoint_shard_leaves_insert(&[checkpoint_leaf(4)])
                })
                .await
                .unwrap();

            assert_eq!(checkpoint_leaves_above(&store, 0, 3).await, [2, 4, 6]);
            assert_eq!(checkpoint_leaves_above(&store, 4, 3).await, [6, 9]);

            promote(&store, &progress, 4).await.unwrap();
            assert_eq!(checkpoint_leaves_above(&store, 0, 10).await, [6, 9]);
        }

        #[test]
        fn rewinding_a_shard_nothing_is_verified_of_restarts_it_from_its_first_version() {
            let mut progress = SyncProgress::default();
            progress.record_state_version(SHARD, v(7), Epoch(1));
            progress.rewind_to_verified(SHARD);
            assert_eq!(cursors_for(&[SHARD], &progress)[0].start_state_version, 1);
        }

        #[test]
        fn a_staged_version_decodes_to_the_updates_of_every_chunk() {
            let buf = VersionBuf::decode_staged(SHARD, v(1), &encoded_receipt_version(1, 1, 10)).unwrap();
            assert_eq!(buf.transactions.len(), 2);
            assert!(VersionBuf::decode_staged(SHARD, v(1), &[0xff, 0xff]).is_err());
        }
    }

    mod served_checkpoints {
        use indexmap::IndexMap;
        use tari_ootle_storage::consensus_models::EndOfEpochCommand;
        use tari_sidechain::{CommandCommitProof, FixedHash, SidechainBlockCommitProof, SidechainBlockHeader};
        use tari_state_tree::{TreeHash, compute_proof_for_hashes};

        use super::*;

        fn checkpoint(epoch: u64, total_exhaust_burn: u128) -> EpochCheckpoint {
            let key = TreeHash::new([1; 32]);
            let (_, inclusion_proof) = compute_proof_for_hashes([key].into_iter(), key).unwrap();
            let commit_proof = SidechainBlockCommitProof {
                header: SidechainBlockHeader {
                    network: 0,
                    protocol_version: 0,
                    parent_id: FixedHash::zero(),
                    justify_id: FixedHash::zero(),
                    height: 0,
                    epoch,
                    epoch_hash: FixedHash::zero(),
                    shard_group: tari_sidechain::ShardGroup {
                        start: 0,
                        end_inclusive: 255,
                    },
                    proposed_by: Default::default(),
                    state_merkle_root: FixedHash::zero(),
                    command_merkle_root: FixedHash::zero(),
                    transaction_merkle_root: FixedHash::zero(),
                    signature: Default::default(),
                    accumulated_data: tari_sidechain::ShardGroupAccumulatedData { total_exhaust_burn },
                    metadata_hash: FixedHash::zero(),
                },
                proof_elements: vec![],
            };
            EpochCheckpoint::new(
                CommandCommitProof::new(EndOfEpochCommand::new(FixedHash::zero()), commit_proof, inclusion_proof),
                IndexMap::new(),
                tari_ootle_storage::consensus_models::BlockHeaderMetadata {
                    total_leader_fee: 0,
                    timestamp: 0,
                    extra_data: Default::default(),
                    timeout_certificate_id: None,
                    is_dummy: false,
                },
            )
        }

        #[test]
        fn a_burn_the_supply_could_pay_is_accepted() {
            let burn = checkpoint_exhaust_burn(&checkpoint(3, u128::from(u64::MAX))).unwrap();
            assert_eq!(burn, Amount::from(u64::MAX));
        }

        #[test]
        fn a_burn_beyond_the_supply_is_rejected() {
            let result = checkpoint_exhaust_burn(&checkpoint(3, u128::from(u64::MAX) + 1));
            assert!(
                matches!(result, Err(NetworkStateSyncError::InvalidCheckpoint { .. })),
                "{result:?}"
            );
        }

        /// The head checkpoint is stored ahead of the backfill without moving its progress, and the backfill that
        /// reaches it later records its progress without counting its exhaust burn a second time.
        #[tokio::test]
        async fn a_head_checkpoint_stored_ahead_of_the_backfill_is_counted_once() {
            use crate::store::IndexerStore;

            let dir = tempfile::tempdir().unwrap();
            let store = SqliteIndexerStore::try_create(dir.path().join("indexer.db")).unwrap();
            let head = checkpoint(250, 7);
            let shard_group = head.checked_shard_group().unwrap();
            let burn = checkpoint_exhaust_burn(&head).unwrap();

            let ahead = head.clone();
            store
                .with_write_tx(move |tx| write_checkpoint(tx, &ahead, shard_group, burn, &[], None))
                .await
                .unwrap();
            let progress = store
                .with_read_tx(|tx| tx.key_value_get_value::<_, SyncProgress>(Key::SyncProgress))
                .await
                .optional()
                .unwrap();
            assert!(progress.is_none());

            let mut backfilled = SyncProgress::default();
            backfilled.record_checkpoint(shard_group, Epoch(250));
            store
                .with_write_tx(move |tx| write_checkpoint(tx, &head, shard_group, burn, &[], Some(backfilled)))
                .await
                .unwrap();
            let progress = store
                .with_read_tx(|tx| tx.key_value_get_value::<_, SyncProgress>(Key::SyncProgress))
                .await
                .unwrap();
            assert_eq!(progress.checkpoint_epoch(shard_group), Some(Epoch(250)));
            let total = store
                .with_read_tx(|tx| tx.key_value_get_value::<_, Amount>(Key::TariAccumulatedExhaustBurn))
                .await
                .unwrap();
            assert_eq!(total, Amount::from(7u64));
        }

        #[test]
        fn the_previous_epochs_checkpoint_is_synced_first_only_when_earlier_ones_are_missing() {
            assert!(is_backfill(Epoch(0), Epoch(250)));
            assert!(is_backfill(Epoch(248), Epoch(250)));
            assert!(!is_backfill(Epoch(249), Epoch(250)));
            assert!(!is_backfill(Epoch(250), Epoch(250)));
        }

        #[test]
        fn only_checkpoints_from_the_synced_epoch_to_the_previous_epoch_are_kept() {
            let mut checkpoints = (1..=6).map(|epoch| checkpoint(epoch, 0)).collect();

            retain_checkpoints_in_sync_range(&mut checkpoints, Epoch(2), Epoch(4));

            let epochs = checkpoints.iter().map(|c| c.epoch().as_u64()).collect::<Vec<_>>();
            assert_eq!(epochs, [2, 3, 4]);
        }
    }

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

        fn order(cursors: &[(u32, u64)]) -> StreamOrder {
            StreamOrder::new(
                &cursors
                    .iter()
                    .map(|&(shard, start_state_version)| rpc::ShardCursor {
                        shard,
                        start_state_version,
                    })
                    .collect::<Vec<_>>(),
            )
        }

        fn v(n: u64) -> StateVersion {
            StateVersion::new(n)
        }

        #[test]
        fn it_accepts_shards_streamed_one_after_another() {
            let mut o = order(&[(1, 1), (2, 5)]);
            o.accept_batch(S1, v(1), false, 1, 0).unwrap();
            o.accept_batch(S1, v(2), false, 1, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(2)).unwrap(), (v(2), true));
            o.accept_batch(S2, v(5), false, 1, 0).unwrap();
            assert_eq!(o.accept_marker(S2, v(5)).unwrap(), (v(5), true));
        }

        #[test]
        fn it_accepts_a_version_split_across_chunks() {
            let mut o = order(&[(1, 3)]);
            o.accept_batch(S1, v(3), true, 1, 0).unwrap();
            o.accept_batch(S1, v(3), true, 1, 0).unwrap();
            o.accept_batch(S1, v(3), false, 1, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(3)).unwrap(), (v(3), true));
        }

        #[test]
        fn it_rejects_a_version_below_the_cursor() {
            // A cursor of 5 asks to resume at v5, so v5 is wanted and everything under it is already held.
            assert!(order(&[(1, 5)]).accept_batch(S1, v(3), false, 1, 0).is_err());
            assert!(order(&[(1, 5)]).accept_batch(S1, v(4), false, 1, 0).is_err());
            order(&[(1, 5)]).accept_batch(S1, v(5), false, 1, 0).unwrap();
        }

        #[test]
        fn it_rejects_a_replayed_or_regressing_version() {
            let mut o = order(&[(1, 9)]);
            o.accept_batch(S1, v(9), false, 1, 0).unwrap();
            o.accept_batch(S1, v(10), false, 1, 0).unwrap();
            assert!(o.accept_batch(S1, v(10), false, 1, 0).is_err());
            assert!(o.accept_batch(S1, v(9), false, 1, 0).is_err());
        }

        #[test]
        fn it_rejects_a_skipped_version() {
            assert!(order(&[(1, 1)]).accept_batch(S1, v(2), false, 1, 0).is_err());
            assert!(order(&[(1, 1)]).accept_batch(S1, v(2), true, 1, 0).is_err());
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(1), false, 1, 0).unwrap();
            assert!(o.accept_batch(S1, v(3), false, 1, 0).is_err());
        }

        #[test]
        fn it_rejects_a_marker_past_the_last_version_streamed() {
            let mut o = order(&[(1, 101)]);
            assert!(o.accept_marker(S1, v(100_000)).is_err());
            assert!(o.accept_marker(S1, v(101)).is_err());
            o.accept_batch(S1, v(101), false, 1, 0).unwrap();
            assert!(o.accept_marker(S1, v(102)).is_err());
            assert_eq!(o.accept_marker(S1, v(101)).unwrap(), (v(101), true));
        }

        #[test]
        fn it_accepts_a_shard_streamed_again_after_its_marker() {
            let mut o = order(&[(1, 2), (2, 1)]);
            o.accept_batch(S1, v(2), false, 1, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(2)).unwrap(), (v(2), true));
            assert_eq!(o.accept_marker(S2, v(0)).unwrap(), (v(0), false));
            o.accept_batch(S1, v(3), false, 1, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(3)).unwrap(), (v(3), true));
            // A forced marker on an epoch change closes off nothing new.
            assert_eq!(o.accept_marker(S1, v(3)).unwrap(), (v(3), false));
            assert!(o.accept_batch(S1, v(3), false, 1, 0).is_err());
        }

        #[test]
        fn it_rejects_an_interleaved_shard_mid_version() {
            let mut o = order(&[(1, 3), (2, 1)]);
            o.accept_batch(S1, v(3), true, 1, 0).unwrap();
            assert!(o.accept_batch(S2, v(1), false, 1, 0).is_err());
        }

        #[test]
        fn it_rejects_a_marker_while_a_version_is_incomplete() {
            let mut o = order(&[(1, 3)]);
            o.accept_batch(S1, v(3), true, 1, 0).unwrap();
            assert!(o.accept_marker(S1, v(3)).is_err());
        }

        #[test]
        fn it_rejects_unrequested_shards() {
            assert!(order(&[(1, 1)]).accept_batch(S2, v(1), false, 1, 0).is_err());
            assert!(order(&[(1, 1)]).accept_marker(S2, v(1)).is_err());
        }

        #[test]
        fn it_rejects_a_batch_with_no_updates() {
            assert!(order(&[(1, 1)]).accept_batch(S1, v(1), false, 0, 0).is_err());
            assert!(order(&[(1, 1)]).accept_batch(S1, v(1), true, 0, 0).is_err());
            // Empty versions cannot walk a stream's cursor forward ahead of a marker.
            let mut o = order(&[(1, 1)]);
            o.accept_batch(S1, v(1), false, 1, 0).unwrap();
            assert!(o.accept_batch(S1, v(2), false, 0, 0).is_err());
            assert_eq!(o.accept_marker(S1, v(1)).unwrap(), (v(1), true));
            assert!(o.accept_marker(S1, v(2)).is_err());
        }

        #[test]
        fn it_accepts_the_last_representable_version() {
            let mut o = order(&[(1, u64::MAX)]);
            o.accept_batch(S1, v(u64::MAX), false, 1, 0).unwrap();
            assert_eq!(o.accept_marker(S1, v(u64::MAX)).unwrap(), (v(u64::MAX), true));
            assert!(o.accept_batch(S1, v(u64::MAX), false, 1, 0).is_err());
        }

        #[test]
        fn it_rejects_a_version_streamed_past_the_byte_budget() {
            const CHUNK: usize = 6 * 1024 * 1024;
            let mut o = order(&[(1, 3)]);
            for _ in 0..MAX_BUFFERED_VERSION_BYTES / CHUNK {
                o.accept_batch(S1, v(3), true, 1, CHUNK).unwrap();
            }
            assert!(o.accept_batch(S1, v(3), true, 1, CHUNK).is_err());
        }

        #[test]
        fn the_byte_budget_applies_to_each_version_afresh() {
            let mut o = order(&[(1, 3)]);
            o.accept_batch(S1, v(3), true, 1, MAX_BUFFERED_VERSION_BYTES - 1)
                .unwrap();
            o.accept_batch(S1, v(3), false, 1, 1).unwrap();
            o.accept_batch(S1, v(4), true, 1, MAX_BUFFERED_VERSION_BYTES - 1)
                .unwrap();
            assert!(o.accept_batch(S1, v(4), false, 1, 2).is_err());
        }
    }

    mod stream_checks {
        use super::*;

        const S1: Shard = Shard::from_u32(1);
        const S2: Shard = Shard::from_u32(2);

        fn v(n: u64) -> StateVersion {
            StateVersion::new(n)
        }

        #[tokio::test(start_paused = true)]
        async fn a_version_no_leaf_reaches_within_the_grace_is_overdue() {
            let mut checks = StreamChecks::default();
            checks.stage(S1, v(5), 10, time::Instant::now());
            time::advance(Duration::from_secs(1)).await;
            checks.stage(S2, v(1), 10, time::Instant::now());

            time::advance(VERIFICATION_GRACE).await;
            assert_eq!(checks.overdue(time::Instant::now()), Some((S1, v(5))));

            checks.cover(S1, v(5));
            assert_eq!(checks.overdue(time::Instant::now()), None);
            time::advance(Duration::from_secs(1)).await;
            assert_eq!(checks.overdue(time::Instant::now()), Some((S2, v(1))));
        }

        #[tokio::test(start_paused = true)]
        async fn a_leaf_below_the_staged_versions_covers_only_those_up_to_it() {
            let mut checks = StreamChecks::default();
            for n in 1..=3 {
                checks.stage(S1, v(n), 100, time::Instant::now());
            }
            checks.cover(S1, v(2));
            assert_eq!((checks.uncovered_versions, checks.uncovered_bytes), (1, 100));

            time::advance(VERIFICATION_GRACE + Duration::from_secs(1)).await;
            assert_eq!(checks.overdue(time::Instant::now()), Some((S1, v(3))));
        }

        #[test]
        fn the_stream_is_full_at_either_cap() {
            let now = time::Instant::now();
            let mut checks = StreamChecks::default();
            checks.stage(S1, v(1), MAX_UNCOVERED_BYTES - 1, now);
            assert!(!checks.is_full());
            checks.stage(S1, v(2), 1, now);
            assert!(checks.is_full());
            checks.cover(S1, v(2));
            assert!(!checks.is_full());

            for n in 0..MAX_UNCOVERED_VERSIONS as u64 {
                checks.stage(S2, v(n + 1), 0, now);
            }
            assert!(checks.is_full());
        }

        #[tokio::test(start_paused = true)]
        async fn a_leaf_ahead_of_a_level_shard_for_the_grace_is_withheld() {
            let mut checks = StreamChecks::default();
            let level = HashMap::from([(S1, v(4)), (S2, v(9))]);
            let waiting = |shard| (shard == S1).then_some(v(6));

            assert_eq!(checks.withheld(&level, waiting, time::Instant::now()), None);
            time::advance(VERIFICATION_GRACE).await;
            assert_eq!(checks.withheld(&level, waiting, time::Instant::now()), None);
            time::advance(Duration::from_secs(1)).await;
            assert_eq!(
                checks.withheld(&level, waiting, time::Instant::now()),
                Some((S1, v(4), v(6)))
            );
        }

        #[tokio::test(start_paused = true)]
        async fn a_shard_that_streams_again_restarts_its_grace() {
            let mut checks = StreamChecks::default();
            let waiting = |shard| (shard == S1).then_some(v(6));
            checks.withheld(&HashMap::from([(S1, v(4))]), waiting, time::Instant::now());
            time::advance(VERIFICATION_GRACE).await;

            // Not level while it streams the versions up to the leaf.
            checks.withheld(&HashMap::new(), waiting, time::Instant::now());
            time::advance(Duration::from_secs(1)).await;
            let level = HashMap::from([(S1, v(5))]);
            assert_eq!(checks.withheld(&level, waiting, time::Instant::now()), None);
            assert_eq!(
                checks.withheld(&level, |_| None, time::Instant::now() + VERIFICATION_GRACE * 2),
                None
            );
        }
    }

    mod mismatch_breaker {
        use tari_crypto::{
            keys::PublicKey,
            ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        };

        use super::*;

        const SHARD: Shard = Shard::from_u32(1);

        fn peer(n: u64) -> PeerAddress {
            RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(n)).into()
        }

        #[test]
        fn it_trips_once_every_member_mismatched_from_the_same_version() {
            let mut breaker = MismatchBreaker::default();
            assert!(!breaker.record(SHARD, StateVersion::new(4), &[peer(1)], 3));
            assert!(!breaker.record(SHARD, StateVersion::new(4), &[peer(1)], 3));
            assert!(!breaker.record(SHARD, StateVersion::new(5), &[peer(2)], 3));
            assert!(!breaker.record(Shard::from_u32(2), StateVersion::new(4), &[peer(2)], 3));
            assert!(breaker.record(SHARD, StateVersion::new(4), &[peer(2), peer(3)], 3));
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
        fn a_shard_at_the_largest_version_gets_no_cursor() {
            let mut progress = progress_at(u64::MAX);
            progress.record_state_version(Shard::from_u32(2), StateVersion::new(u64::MAX - 1), Epoch(1));
            let cursors = cursors_for(&[S1, Shard::from_u32(2)], &progress);
            assert_eq!(cursors.len(), 1);
            assert_eq!(cursors[0].shard, 2);
            assert_eq!(cursors[0].start_state_version, u64::MAX);
        }
    }

    mod checkpoint_verification {
        use std::{
            collections::{HashMap, HashSet},
            sync::Arc,
        };

        use tari_epoch_manager::EpochManagerError;
        use tari_ootle_common_types::{
            SubstateAddress,
            VotePower,
            committee::{Committee, CommitteeInfo, CommitteeMember},
        };
        use tari_ootle_storage::{
            consensus_models::{BlockHeaderMetadata, EndOfEpochCommand},
            global::models::ValidatorNode,
        };
        use tari_sidechain::{CommandCommitProof, FixedHash, SidechainBlockCommitProof, SidechainBlockHeader};
        use tari_state_tree::{TreeHash, compute_proof_for_hashes};
        use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
        use tokio::sync::broadcast;

        use super::*;

        type Addr = String;

        fn low() -> ShardGroup {
            ShardGroup::new(1u32, 127u32)
        }

        fn high() -> ShardGroup {
            ShardGroup::new(128u32, 255u32)
        }

        /// A well-formed checkpoint that no committee signed.
        fn checkpoint(epoch: u64, shard_group: ShardGroup) -> EpochCheckpoint {
            let state_root = checkpoint_with_state_root(epoch, shard_group, FixedHash::zero())
                .compute_state_merkle_root()
                .unwrap();
            checkpoint_with_state_root(epoch, shard_group, FixedHash::from(state_root.into_array()))
        }

        fn checkpoint_with_state_root(
            epoch: u64,
            shard_group: ShardGroup,
            state_merkle_root: FixedHash,
        ) -> EpochCheckpoint {
            let header_metadata = BlockHeaderMetadata {
                total_leader_fee: 0,
                timestamp: epoch,
                extra_data: Default::default(),
                timeout_certificate_id: None,
                is_dummy: false,
            };
            let key = TreeHash::new([1; 32]);
            let (_, inclusion_proof) = compute_proof_for_hashes([key].into_iter(), key).unwrap();
            let commit_proof = SidechainBlockCommitProof {
                header: SidechainBlockHeader {
                    network: 0,
                    protocol_version: 0,
                    parent_id: FixedHash::zero(),
                    justify_id: FixedHash::zero(),
                    height: 0,
                    epoch,
                    epoch_hash: FixedHash::zero(),
                    shard_group: tari_sidechain::ShardGroup {
                        start: shard_group.start().as_u32(),
                        end_inclusive: shard_group.end().as_u32(),
                    },
                    proposed_by: Default::default(),
                    state_merkle_root,
                    command_merkle_root: FixedHash::zero(),
                    transaction_merkle_root: FixedHash::zero(),
                    signature: Default::default(),
                    accumulated_data: Default::default(),
                    metadata_hash: header_metadata.calculate_hash(),
                },
                proof_elements: vec![],
            };
            EpochCheckpoint::new(
                CommandCommitProof::new(EndOfEpochCommand::new(FixedHash::zero()), commit_proof, inclusion_proof),
                Default::default(),
                header_metadata,
            )
        }

        fn committee() -> Arc<Committee<Addr>> {
            Arc::new(Committee::new(vec![CommitteeMember {
                address: "member".to_string(),
                public_key: RistrettoPublicKeyBytes::default(),
                vote_power: VotePower::of(1),
            }]))
        }

        /// The epochs an epoch manager has a record of, each with the committees of its shard groups.
        #[derive(Default)]
        struct EpochRecords {
            committees: HashMap<Epoch, Vec<ShardGroup>>,
            validator_set_hash: Option<ValidatorSetHash>,
        }

        impl EpochRecords {
            fn with(mut self, epoch: u64, shard_groups: &[ShardGroup]) -> Self {
                self.committees.insert(Epoch(epoch), shard_groups.to_vec());
                self
            }
        }

        impl EpochManagerReader for EpochRecords {
            type Addr = Addr;

            fn subscribe(&self) -> broadcast::Receiver<EpochManagerEvent> {
                unimplemented!()
            }

            async fn wait_for_initial_scanning_to_complete(&self) -> Result<(), EpochManagerError> {
                unimplemented!()
            }

            async fn get_all_validator_nodes(&self, _: Epoch) -> Result<Vec<ValidatorNode<Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_info_by_validator_address(
                &self,
                _: Epoch,
                _: &Addr,
            ) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_for_substate(
                &self,
                _: Epoch,
                _: SubstateAddress,
            ) -> Result<Arc<Committee<Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_validator_node_by_public_key(
                &self,
                _: Epoch,
                _: RistrettoPublicKeyBytes,
            ) -> Result<ValidatorNode<Addr>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_our_validator_node(&self, _: Epoch) -> Result<ValidatorNode<Addr>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_local_committee_info(&self, _: Epoch) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_info(&self, _: Epoch, _: ShardGroup) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_info_for_substate(
                &self,
                _: Epoch,
                _: SubstateAddress,
            ) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn current_epoch(&self) -> Result<Epoch, EpochManagerError> {
                unimplemented!()
            }

            async fn get_current_epoch_hash(&self) -> Result<FixedHash, EpochManagerError> {
                unimplemented!()
            }

            async fn get_epoch_hash(&self, epoch: Epoch) -> Result<FixedHash, EpochManagerError> {
                if self.committees.contains_key(&epoch) {
                    Ok(FixedHash::zero())
                } else {
                    Err(EpochManagerError::NoEpochFound(epoch))
                }
            }

            async fn get_validator_set_hash(&self, epoch: Epoch) -> Result<ValidatorSetHash, EpochManagerError> {
                self.validator_set_hash
                    .ok_or(EpochManagerError::NoValidatorSet { epoch })
            }

            async fn get_validator_set_committee_proof(
                &self,
                _: Epoch,
                _: ShardGroup,
            ) -> Result<tari_state_tree::ValidatorSetCommitteeProof, EpochManagerError> {
                unimplemented!()
            }

            async fn get_num_committees(&self, _: Epoch) -> Result<u32, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_by_shard_group(
                &self,
                epoch: Epoch,
                shard_group: ShardGroup,
            ) -> Result<Arc<Committee<Addr>>, EpochManagerError> {
                match self.committees.get(&epoch) {
                    Some(shard_groups) if shard_groups.contains(&shard_group) => Ok(committee()),
                    _ => Err(EpochManagerError::NoEpochFound(epoch)),
                }
            }

            async fn get_committees_overlapping_shard_group(
                &self,
                _: Epoch,
                _: ShardGroup,
            ) -> Result<HashMap<ShardGroup, Committee<Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_random_committee_member(
                &self,
                _: Epoch,
                _: Option<ShardGroup>,
                _: HashSet<Addr>,
            ) -> Result<ValidatorNode<Addr>, EpochManagerError> {
                unimplemented!()
            }

            async fn lock_epoch(&self, _: Epoch) -> Result<(), EpochManagerError> {
                unimplemented!()
            }

            async fn get_observed_epoch_hash(&self, _: Epoch) -> Result<Option<FixedHash>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_birthday_epoch(&self) -> Result<Option<Epoch>, EpochManagerError> {
                unimplemented!()
            }
        }

        fn is_invalid_checkpoint(result: &Result<CheckpointCheck, NetworkStateSyncError>) -> bool {
            matches!(result, Err(NetworkStateSyncError::InvalidCheckpoint { .. }))
        }

        #[tokio::test]
        async fn a_checkpoint_before_the_records_is_checked_for_form_only() {
            let records = EpochRecords::default().with(5, &[low(), high()]);

            let result = verify_checkpoint(Network::MainNet, &records, &checkpoint(2, low()), low(), Epoch(5)).await;

            assert_eq!(result.unwrap(), CheckpointCheck::FormOnly);
        }

        #[tokio::test]
        async fn an_older_recorded_checkpoint_is_checked_against_its_committee() {
            let records = EpochRecords::default()
                .with(3, &[low(), high()])
                .with(5, &[low(), high()]);

            let result = verify_checkpoint(Network::MainNet, &records, &checkpoint(3, low()), low(), Epoch(5)).await;

            assert!(is_invalid_checkpoint(&result), "{result:?}");
        }

        /// A shard group is one the peer names, so naming one the epoch did not have must not skip the committee check.
        #[tokio::test]
        async fn a_shard_group_outside_a_recorded_epoch_is_rejected() {
            let records = EpochRecords::default()
                .with(3, &[low(), high()])
                .with(5, &[low(), high()]);
            let all = ShardGroup::new(1u32, 255u32);

            for epoch in [3, 5] {
                let result =
                    verify_checkpoint(Network::MainNet, &records, &checkpoint(epoch, all), all, Epoch(5)).await;
                assert!(is_invalid_checkpoint(&result), "{epoch}: {result:?}");
                assert!(result.unwrap_err().is_peer_fault());
            }
        }

        #[test]
        fn a_next_epoch_set_other_than_the_local_one_diverges() {
            let local = ValidatorSetHash::from_array([1; 32]);
            let committed = ValidatorSetHash::from_array([2; 32]);

            assert!(check_validator_set(Epoch(4), local, local).is_ok());
            assert!(matches!(
                check_validator_set(Epoch(4), local, committed),
                Err(NetworkStateSyncError::ValidatorSetDiverged { epoch, .. }) if epoch == Epoch(4)
            ));
        }
    }

    mod sync_loop {
        use super::*;
        use crate::{storage_sqlite::temp_store, store::IndexerStoreWriteTransaction};

        async fn write(store: &SqliteIndexerStore, value: u64) -> Result<(), StorageError> {
            store
                .with_write_tx(move |tx| tx.key_value_set(Key::TariAccumulatedFees, value))
                .await
        }

        /// The shape the sync loop's `select!` keeps out: `sync` queued on the store's write lock while a
        /// branch body awaits a write of its own. The lock is handed to `sync`, which goes unpolled until the
        /// body ends, so the body's write waits on a lock nothing takes and only the write lock timeout
        /// ends the wait.
        #[tokio::test(start_paused = true)]
        async fn a_branch_writing_while_the_pinned_sync_is_queued_waits_out_the_write_lock_timeout() {
            let (_dir, store) = temp_store().await;
            let held = store.hold_write_lock().await;
            let sync_store = store.clone();
            let mut sync = pin!(async move { write(&sync_store, 1).await });
            assert!(futures::poll!(sync.as_mut()).is_pending());
            drop(held);

            let started = time::Instant::now();
            let mut branch_write = pin!(write(&store, 2));
            assert!(
                time::timeout(Duration::from_secs(10), &mut branch_write).await.is_err(),
                "the branch's write completed though the lock was handed to an unpolled waiter"
            );
            let err = branch_write.await.unwrap_err();
            assert_eq!(started.elapsed(), Duration::from_secs(30));
            assert!(
                err.to_string().contains("waiting for the indexer store's write lock"),
                "{err}"
            );

            sync.await.unwrap();
        }
    }
}
