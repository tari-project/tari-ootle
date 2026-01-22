//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, pin::pin};

use futures::StreamExt;
use log::*;
use tari_engine_types::{
    substate::{SubstateId, SubstateValue},
    transaction_receipt::TransactionReceipt,
};
use tari_epoch_manager::{service::EpochManagerHandle, EpochManagerEvent, EpochManagerReader};
use tari_indexer_client::event::{IndexerEvent, NewEpochEvent, TransactionFinalizedEvent};
use tari_networking::NetworkingHandle;
use tari_ootle_common_types::{
    optional::Optional,
    shard::Shard,
    Epoch,
    PeerAddress,
    ShardGroup,
    StateVersion,
    VotePower,
};
use tari_ootle_p2p::{proto::rpc, TariMessagingSpec};
use tari_ootle_storage::{
    consensus_models::{EpochCheckpoint, SubstateData, SubstateUpdateProof, SubstateValueFilterFlags},
    StorageError,
};
use tari_ootle_transaction::TransactionId;
use tari_rpc_framework::__macro_reexports::future::Either;
use tari_shutdown::ShutdownSignal;
use tari_template_lib_types::{Amount, TransactionReceiptAddress};
use tokio::{sync::broadcast, time};

use crate::{
    network_state_sync::{
        committee_client::{ValidatorCommitteeRpcPool, ValidatorRpcSession},
        config::NetworkWideStateSyncConfig,
        error::NetworkStateSyncError,
        stats::SyncStats,
        sync_plan::SyncPlan,
        sync_progress::SyncProgress,
    },
    notify::Notify,
    storage_sqlite::{
        models::{Key, UtxoSpent, UtxoUnspent, UtxoUpdateRecord},
        SqliteIndexerStore,
        SqliteStoreWriteTransaction,
    },
    store::{IndexerStore, IndexerStoreReadTransaction, IndexerStoreReader, IndexerStoreWriteTransaction},
};

const LOG_TARGET: &str = "tari::indexer::network_state_sync::worker";

#[derive(Clone)]
pub struct NetworkWideStateSync {
    epoch_manager: EpochManagerHandle<PeerAddress>,
    networking: NetworkingHandle<TariMessagingSpec>,
    store: SqliteIndexerStore,
    stats: SyncStats,
    config: NetworkWideStateSyncConfig,
    notify: Notify<IndexerEvent>,
}

impl NetworkWideStateSync {
    pub fn new(
        epoch_manager: EpochManagerHandle<PeerAddress>,
        networking: NetworkingHandle<TariMessagingSpec>,
        storage: SqliteIndexerStore,
        config: NetworkWideStateSyncConfig,
        notify: Notify<IndexerEvent>,
    ) -> Self {
        Self {
            epoch_manager,
            networking,
            store: storage,
            stats: SyncStats::new(),
            config,
            notify,
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
            info!(target: LOG_TARGET, "🌍️ Current epoch is zero, skipping sync round.");
            return Ok(());
        }
        self.start_sync(sync_plan).await?;
        self.stats.log_stats();
        self.stats.reset();
        Ok(())
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
            let checkpoints: Vec<_> = pool
                .try_with_random_members(|mut session| async move {
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
                            EpochCheckpoint::try_from(cp).map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                                details: format!("Failed to convert checkpoint for shard group {}: {}", shard_group, e),
                            })
                        })
                        .collect()
                })
                .await?;

            if checkpoints.is_empty() {
                info!(target: LOG_TARGET, "🌍️ No checkpoints found for shard group {shard_group} from epoch {from_epoch} (prev_epoch {prev_epoch})");
                sync_plan_mut.add_checkpoint_sync_progress(shard_group, prev_epoch);
                self.store
                    .with_write_tx(|tx| tx.key_value_set(Key::SyncProgress, sync_plan_mut.sync_progress()))?;
                continue;
            }

            info!(target: LOG_TARGET, "🌍️ Found {} checkpoints for shard group {shard_group} from epoch {from_epoch}", checkpoints.len());

            let committee = self
                .epoch_manager
                .get_committee_by_shard_group(prev_epoch, shard_group, None, false)
                .await?;

            // TODO: continue on failure
            for checkpoint in checkpoints {
                info!(target: LOG_TARGET, "🌍️ Validating checkpoint for shard group {shard_group}: {}", checkpoint.header().calculate_hash());
                // TODO: we require historical committees to validate older checkpoints. Figure out the best way to
                //       avoid needing the data (e.g. VN merkle inclusion proof + historic L1 block MR), or,
                //       decide it is ok to require this data to be locally stored by all indexers. For now, to avoid
                //       complexity that may be removed later, we'll skip validating them and only validate prev_epochs
                //       checkpoint.
                let checkpoint_shard_group =
                    checkpoint
                        .checked_shard_group()
                        .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                            details: format!("Checkpoint for shard group {} is not valid: {}", shard_group, e),
                        })?;

                if checkpoint.epoch() == prev_epoch {
                    checkpoint
                        .validate(checkpoint.epoch(), committee.quorum_threshold(), |pk| {
                            Ok(committee.get_power_by_public_key(pk).unwrap_or_else(VotePower::zero))
                        })
                        .map_err(|e| NetworkStateSyncError::InvalidCheckpoint {
                            details: format!("Failed to validate checkpoint for shard group {}: {}", shard_group, e),
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
                self.store.with_write_tx(|tx| {
                    if !tx.epoch_checkpoint_exists(shard_group, checkpoint.epoch())? {
                        tx.insert_or_ignore_epoch_checkpoint(&checkpoint)?;

                        let exhausted = tx
                            .key_value_get_value::<_, Amount>(Key::XtrAccumulatedExhaustBurn)
                            .optional()?;

                        let new_exhausted = exhausted.unwrap_or_else(Amount::zero) + xtr_exhausted;
                        tx.key_value_set(Key::XtrAccumulatedExhaustBurn, new_exhausted)?;
                    }
                    tx.key_value_set(Key::SyncProgress, sync_plan_mut.sync_progress())
                })?;
            }
        }

        Ok(())
    }

    async fn sync_state(&mut self, sync_plan_mut: &mut SyncPlan) -> Result<(), NetworkStateSyncError> {
        let committee_pools = sync_plan_mut.committee_pools().clone();
        let mut update_buf = Vec::new();
        let mut utxos_buf = Vec::new();
        let mut transactions_buf = Vec::new();
        let mut validator_fee_pools_buf = Vec::new();

        let mut has_synced_global_shard = false;

        for (shard_group, mut pool) in committee_pools {
            // TODO: make this robust against failures, e.g. if one peer fails, continue with others
            // TODO: consider syncing shards in epoch chunks rather than one after another
            // TODO: consider parallelizing shard syncs within a shard group
            let mut session = pool.new_session().await?;
            if !has_synced_global_shard {
                self.sync_shard_state(
                    Shard::global(),
                    sync_plan_mut,
                    &mut update_buf,
                    &mut utxos_buf,
                    &mut transactions_buf,
                    &mut validator_fee_pools_buf,
                    shard_group,
                    &mut session,
                )
                .await?;
                has_synced_global_shard = true;
            }

            for shard in shard_group.shard_iter() {
                self.sync_shard_state(
                    shard,
                    sync_plan_mut,
                    &mut update_buf,
                    &mut utxos_buf,
                    &mut transactions_buf,
                    &mut validator_fee_pools_buf,
                    shard_group,
                    &mut session,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn sync_shard_state(
        &mut self,
        shard: Shard,
        sync_plan_mut: &mut SyncPlan,
        update_buf: &mut Vec<(Epoch, SubstateUpdateProof)>,
        utxos_buf: &mut Vec<UtxoUpdateRecord>,
        transactions_buf: &mut Vec<(TransactionReceiptAddress, TransactionReceipt)>,
        validator_fee_pools_buf: &mut Vec<SubstateData>,
        shard_group: ShardGroup,
        session: &mut ValidatorRpcSession,
    ) -> Result<(), NetworkStateSyncError> {
        // Perform sync operations using the pool and state
        let (prev_version, prev_epoch) = sync_plan_mut
            .sync_progress()
            .last_state_versions
            .get(&shard)
            .map(|(v, e)| (v.as_u64(), *e))
            .unwrap_or_else(|| (0, Epoch::zero()));
        let from_version = prev_version + 1;
        let mut value_filters = SubstateValueFilterFlags::UTXO |
            SubstateValueFilterFlags::VALIDATOR_FEE_POOL |
            SubstateValueFilterFlags::CLAIMED_OUTPUT_TOMBSTONE |
            SubstateValueFilterFlags::TRANSACTION_RECEIPT;

        if prev_version == 0 {
            // If we are syncing from scratch, only get up states to reduce initial sync size
            value_filters |= SubstateValueFilterFlags::UP_ONLY;
        }

        let mut stream = session
            .sync_state(rpc::SyncStateRequest {
                start_state_version: from_version,
                shard: shard.as_u32(),
                // Sync to latest epoch
                until_epoch: None,
                value_filters: value_filters.bits(),
            })
            .await?;

        let mut is_first_iter = true;
        let mut xtr_claimed = Amount::zero();
        while let Some(result) = stream.next().await {
            if is_first_iter {
                // Avoid log spam, only log once per stream
                debug!(target: LOG_TARGET, "🌍️ Established stream for {shard} in shard group {shard_group} from peer {} (last sync: {prev_epoch} {prev_version})", session.peer_address());
                is_first_iter = false;
            }
            let msg = result?;
            let msg_epoch = msg
                .epoch
                .map(Epoch::from)
                .ok_or_else(|| NetworkStateSyncError::InvalidStateUpdate {
                    details: "Received state update without epoch".to_string(),
                })?;
            let state_version = StateVersion::new(msg.state_version);

            for update in msg.updates {
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
                    update_buf,
                    utxos_buf,
                    transactions_buf,
                    validator_fee_pools_buf,
                    &mut xtr_claimed,
                )?;
            }
            if msg.has_more {
                debug!(target: LOG_TARGET, "🌍️ more updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})");
                continue;
            }

            info!(target: LOG_TARGET, "🌍️ Received {} updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})", update_buf.len());

            self.stats.increase_state_updates(update_buf.len());

            self.store.clone().with_write_tx(|tx| {
                debug!(target: LOG_TARGET, "✅ Committing {} updates for shard {shard} (epoch: {msg_epoch}, state version: {state_version})", update_buf.len());
                // TODO: this is not currently used. Consider removing. 
                tx.batch_insert_substate_transitions(shard, state_version, update_buf.drain(..))?;
                debug!(target: LOG_TARGET, "✅ Committing {} UTXOs for shard {shard} (epoch: {msg_epoch})", utxos_buf.len());
                tx.batch_insert_utxo_updates(msg_epoch, utxos_buf.drain(..))?;
                // TODO: there are many ways to do this. This is probably not the best way. This allows wallet to query for validator fee pool values.
                for substate_data in validator_fee_pools_buf.drain(..) {
                    tx.upsert_substate(&substate_data)?;
                }
                debug!(target: LOG_TARGET, "✅ Committing {} transactions for shard {shard} (epoch: {msg_epoch})", transactions_buf.len());
                self.stats.increase_events(transactions_buf.iter().map(|(_, t)| t.events.len()).sum());
                self.persist_transaction_receipts(tx, transactions_buf.drain(..))?;

                // All done - write the sync progress
                sync_plan_mut.add_state_sync_progress(shard, state_version, msg_epoch);
                tx.key_value_set(Key::SyncProgress, sync_plan_mut.sync_progress())?;
                let claimed = tx.key_value_get_value(Key::XtrAccumulatedClaimed).optional()?;
                let new_claimed = claimed.unwrap_or_else(Amount::zero) + xtr_claimed;
                tx.key_value_set(Key::XtrAccumulatedClaimed, new_claimed)
            })?;
        }
        Ok(())
    }

    fn persist_transaction_receipts<I: IntoIterator<Item = (TransactionReceiptAddress, TransactionReceipt)>>(
        &self,
        tx: &mut SqliteStoreWriteTransaction<'_>,
        receipts: I,
    ) -> Result<(), StorageError> {
        tx.batch_insert_transaction_receipts(receipts, &self.config.event_filters)?;
        Ok(())
    }
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
    xtr_claimed_mut: &mut Amount,
) -> Result<(), NetworkStateSyncError> {
    match &update {
        SubstateUpdateProof::Create(create) => match create.substate.value().value() {
            Some(SubstateValue::Utxo(utxo)) => {
                if let Some(address) = create.substate.substate_id().as_utxo_address() {
                    let is_frozen = utxo.is_frozen();
                    if let Some(ref output) = utxo.output {
                        utxos_buf.push(UtxoUpdateRecord::Unspent(UtxoUnspent {
                            address,
                            version: update.version(),
                            shard,
                            state_version,
                            utxo_output: output.clone(),
                            is_frozen,
                        }));
                    }
                } else {
                    warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received UTXO substate with invalid address: {}", create.substate.substate_id());
                };
            },
            Some(SubstateValue::TransactionReceipt(receipt)) => {
                if let Some(address) = update.substate_id().as_transaction_receipt_address() {
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
                if id.is_template() || id.is_transaction_receipt() {
                    warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received substate {id} update with no value");
                }
                if let Some(addr) = id.as_utxo_address() {
                    debug!(target: LOG_TARGET, "🌍️ Received UTXO substate {addr} creation with no value. Ignoring as this means it is spent later.");
                }
            },
        },
        SubstateUpdateProof::Destroy(destroy) => match &destroy.substate_id {
            SubstateId::TransactionReceipt(_) => {
                warn!(target: LOG_TARGET, "⚠️ NEVER HAPPEN: Received destroy for transaction receipt substate: {}", destroy.substate_id);
            },
            SubstateId::Utxo(address) => {
                utxos_buf.push(UtxoUpdateRecord::Spent(UtxoSpent {
                    address: address.clone(),
                    shard,
                    version: update.version(),
                    state_version,
                }));
            },
            _ => {},
        },
    }

    update_buf.push((msg_epoch, update));
    Ok(())
}
