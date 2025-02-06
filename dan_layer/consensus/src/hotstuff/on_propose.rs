//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::Display,
    num::NonZeroU64,
};

use log::*;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    displayable::Displayable,
    optional::Optional,
    shard::Shard,
    Epoch,
    ExtraData,
    NodeHeight,
    VersionedSubstateId,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockHeader,
        BlockId,
        BlockTransactionExecution,
        BurntUtxo,
        Command,
        Decision,
        EvictNodeAtom,
        ForeignProposal,
        ForeignSendCounters,
        HighQc,
        LastProposed,
        LeafBlock,
        LockedBlock,
        PendingShardStateTreeDiff,
        QcId,
        QuorumCertificate,
        SubstateChange,
        TransactionAtom,
        TransactionExecution,
        TransactionPool,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
        ValidatorConsensusStats,
    },
    StateStore,
};
use tari_engine_types::{commit_result::RejectReason, substate::Substate};
use tari_epoch_manager::EpochManagerReader;
use tari_transaction::TransactionId;
use tokio::task;

use crate::{
    hotstuff::{
        apply_leader_fee_to_substate_store,
        block_change_set::ProposedBlockChangeSet,
        calculate_state_merkle_root,
        error::HotStuffError,
        filter_diff_for_committee,
        substate_store::PendingSubstateStore,
        to_public_key_bytes,
        transaction_manager::{
            ConsensusTransactionManager,
            EvidenceOrExecution,
            LocalPreparedTransaction,
            PledgedTransaction,
            PreparedTransaction,
            TransactionLockConflicts,
        },
        HotstuffConfig,
    },
    messages::{HotstuffMessage, ProposalMessage},
    tracing::TraceTimer,
    traits::{ConsensusSpec, OutboundMessaging, ValidatorSignatureService, WriteableSubstateStore},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_local_propose";

struct NextBlock {
    block: Block,
    foreign_proposals: Vec<ForeignProposal>,
    executed_transactions: HashMap<TransactionId, TransactionExecution>,
    lock_conflicts: TransactionLockConflicts,
}

#[derive(Debug, Clone)]
pub struct OnPropose<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
    transaction_manager: ConsensusTransactionManager<TConsensusSpec::TransactionExecutor, TConsensusSpec::StateStore>,
    signing_service: TConsensusSpec::SignatureService,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
}

impl<TConsensusSpec> OnPropose<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        config: HotstuffConfig,
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        transaction_manager: ConsensusTransactionManager<
            TConsensusSpec::TransactionExecutor,
            TConsensusSpec::StateStore,
        >,
        signing_service: TConsensusSpec::SignatureService,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
    ) -> Self {
        Self {
            config,
            store,
            epoch_manager,
            transaction_pool,
            transaction_manager,
            signing_service,
            outbound_messaging,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn handle(
        &mut self,
        epoch: Epoch,
        next_height: NodeHeight,
        local_committee: &Committee<TConsensusSpec::Addr>,
        local_committee_info: CommitteeInfo,
        local_claim_public_key: &PublicKey,
        leaf_block: LeafBlock,
        propose_epoch_end: bool,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::info(LOG_TARGET, "OnPropose");
        if let Some(last_proposed) = self.store.with_read_tx(|tx| LastProposed::get(tx)).optional()? {
            if last_proposed.epoch == epoch && last_proposed.height >= next_height {
                info!(
                    target: LOG_TARGET,
                    "⤵️ SKIPPING propose for {} ({}) because we already proposed block {}",
                    next_height,
                    leaf_block,
                    last_proposed,
                );

                return Ok(());
            }
        }

        let (current_base_layer_block_height, current_base_layer_block_hash) =
            self.epoch_manager.current_base_layer_block_info().await?;

        let base_layer_block_hash = current_base_layer_block_hash;
        let base_layer_block_height = current_base_layer_block_height;

        let on_propose = self.clone();
        let local_claim_public_key = to_public_key_bytes(local_claim_public_key);

        let (next_block, foreign_proposals) = task::spawn_blocking(move || {
            on_propose.store.with_write_tx(|tx| {
                let high_qc = HighQc::get(&**tx, epoch)?;
                let high_qc_cert = high_qc.get_quorum_certificate(&**tx)?;

                info!(
                    target: LOG_TARGET,
                    "🌿 PROPOSE local block with parent {}. HighQC: {}",
                    leaf_block,
                    high_qc_cert,
                );

                let next_block = on_propose.build_next_block(
                    tx,
                    epoch,
                    next_height,
                    leaf_block,
                    high_qc_cert,
                    &local_committee_info,
                    local_claim_public_key,
                    false,
                    base_layer_block_height,
                    base_layer_block_hash,
                    propose_epoch_end,
                )?;

                let NextBlock {
                    block: next_block,
                    foreign_proposals,
                    executed_transactions,
                    lock_conflicts,
                } = next_block;

                lock_conflicts.save_for_block(tx, next_block.id())?;

                // Add executions for this block
                if !executed_transactions.is_empty() {
                    debug!(
                        target: LOG_TARGET,
                        "Saving {} executed transaction(s) for block {}",
                        executed_transactions.len(),
                        next_block.id()
                    );
                }
                for executed in executed_transactions.into_values() {
                    executed.for_block(*next_block.id()).insert_if_required(tx)?;
                }

                next_block.as_last_proposed().set(tx)?;

                Ok::<_, HotStuffError>((next_block, foreign_proposals))
            })
        })
        .await??;

        info!(
            target: LOG_TARGET,
            "🌿 [{}] PROPOSING new local block {} to {} validators. justify: {} ({}), parent: {}",
            self.signing_service.public_key(),
            next_block,
            local_committee.len(),
            next_block.justify().block_id(),
            next_block.justify().block_height(),
            next_block.parent()
        );

        self.broadcast_local_proposal(next_block, foreign_proposals, &local_committee_info)
            .await?;

        Ok(())
    }

    pub async fn broadcast_local_proposal(
        &mut self,
        next_block: Block,
        foreign_proposals: Vec<ForeignProposal>,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let epoch = next_block.epoch();
        let leaf_block = next_block.as_leaf_block();
        let msg = HotstuffMessage::Proposal(ProposalMessage {
            block: next_block,
            foreign_proposals,
        });

        // Broadcast to local and foreign committees
        self.outbound_messaging.send_self(msg.clone()).await?;

        // If we are the only VN in this committee, no need to multicast
        if local_committee_info.num_shard_group_members() <= 1 {
            info!(
                target: LOG_TARGET,
                "🌿 This node is the only member of the local committee. No need to multicast proposal {leaf_block}",
            );
        } else {
            let committee = self
                .epoch_manager
                .get_committee_by_shard_group(epoch, local_committee_info.shard_group(), None)
                .await?;

            info!(
                target: LOG_TARGET,
                "🌿 Broadcasting local proposal to {}/{} local committee members {}",
                committee.len(), local_committee_info.num_shard_group_members(), leaf_block,
            );

            if let Err(err) = self.outbound_messaging.multicast(committee.into_addresses(), msg).await {
                warn!(
                    target: LOG_TARGET,
                    "Failed to multicast proposal to local committee: {}",
                    err
                );
            }
        }

        Ok(())
    }

    /// Returns Ok(None) if the command cannot be sequenced yet due to lock conflicts.
    fn transaction_pool_record_to_command(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        start_of_chain_id: &LeafBlock,
        mut tx_rec: TransactionPoolRecord,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
        executed_transactions: &mut HashMap<TransactionId, TransactionExecution>,
        lock_conflicts: &mut TransactionLockConflicts,
    ) -> Result<Option<Command>, HotStuffError> {
        match tx_rec.current_stage() {
            TransactionPoolStage::New => self.prepare_transaction(
                start_of_chain_id,
                &mut tx_rec,
                local_committee_info,
                substate_store,
                executed_transactions,
                lock_conflicts,
            ),
            // Leader thinks all local nodes have prepared
            TransactionPoolStage::Prepared => {
                if tx_rec.current_decision().is_abort() {
                    let atom = tx_rec.get_current_transaction_atom();
                    return Ok(Some(Command::LocalAccept(atom)));
                }
                if tx_rec
                    .evidence()
                    .is_committee_output_only(local_committee_info.shard_group())
                {
                    if !tx_rec.has_all_required_foreign_input_pledges(tx, local_committee_info)? {
                        error!(
                            target: LOG_TARGET,
                            "BUG: attempted to propose transaction {} as Prepared but not all foreign input pledges were found. \
                             This transaction should not have been marked as ready. {}",
                            tx_rec.transaction_id(),
                            tx_rec.evidence()
                        );
                        return Ok(None);
                    }
                    let atom = tx_rec.get_local_transaction_atom();
                    debug!(
                        target: LOG_TARGET,
                        "ℹ️ Transaction {} is output-only for {}, proposing LocalAccept",
                        tx_rec.transaction_id(),
                        local_committee_info.shard_group()
                    );
                    Ok(Some(Command::LocalAccept(atom)))
                } else {
                    let atom = tx_rec.get_local_transaction_atom();
                    Ok(Some(Command::LocalPrepare(atom)))
                }
            },
            // Leader thinks all foreign PREPARE pledges have been received (condition for LocalPrepared stage to be
            // ready)
            TransactionPoolStage::LocalPrepared => self.all_or_some_prepare_transaction(
                tx,
                start_of_chain_id,
                local_committee_info,
                &mut tx_rec,
                substate_store,
                executed_transactions,
            ),

            // Leader thinks that all local nodes agree that all shard groups have prepared, we are ready to accept
            // locally
            TransactionPoolStage::AllPrepared => Ok(Some(Command::LocalAccept(
                self.get_transaction_atom_with_leader_fee(&mut tx_rec)?,
            ))),
            // Leader thinks local nodes are ready to accept an ABORT
            TransactionPoolStage::SomePrepared => Ok(Some(Command::LocalAccept(tx_rec.get_current_transaction_atom()))),
            // Leader thinks that all foreign ACCEPT pledges have been received and, we are ready to accept the result
            // (COMMIT/ABORT)
            TransactionPoolStage::LocalAccepted => {
                self.accept_transaction(tx, start_of_chain_id, &mut tx_rec, local_committee_info, substate_store)
            },
            // Not reachable as there is nothing to propose for these stages. To confirm that all local nodes
            // agreed with the Accept, more (possibly empty) blocks with QCs will be
            // proposed and accepted, otherwise the Accept block will not be committed.
            TransactionPoolStage::AllAccepted |
            TransactionPoolStage::SomeAccepted |
            TransactionPoolStage::LocalOnly => {
                unreachable!(
                    "It is invalid for TransactionPoolStage::{} to be ready to propose",
                    tx_rec.current_stage()
                )
            },
        }
    }

    fn process_newly_justified_block(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        new_leaf_block: &Block,
        high_qc_id: QcId,
        local_committee_info: &CommitteeInfo,
        change_set: &mut ProposedBlockChangeSet,
    ) -> Result<(), HotStuffError> {
        let locked_block = LockedBlock::get(tx, new_leaf_block.epoch())?;
        info!(
            target: LOG_TARGET,
            "✅ New leaf block {} is justified. Updating evidence for transactions",
            new_leaf_block,
        );

        let leaf = new_leaf_block.as_leaf_block();
        for cmd in new_leaf_block.commands() {
            if !cmd.is_local_prepare() && !cmd.is_local_accept() {
                continue;
            }

            let atom = cmd.transaction().expect("Command must be a transaction");

            let Some(mut pool_tx) = change_set
                .get_transaction(tx, &locked_block, &leaf, atom.id())
                .optional()?
            else {
                return Err(HotStuffError::InvariantError(format!(
                    "Transaction {} in newly justified block {} not found in the pool",
                    atom.id(),
                    leaf,
                )));
            };

            if cmd.is_local_prepare() {
                pool_tx
                    .evidence_mut()
                    .add_shard_group(local_committee_info.shard_group())
                    .set_prepare_qc(high_qc_id);
            } else if cmd.is_local_accept() {
                pool_tx
                    .evidence_mut()
                    .add_shard_group(local_committee_info.shard_group())
                    .set_accept_qc(high_qc_id);
            } else {
                // Nothing
            }

            // Set readiness
            if !pool_tx.is_ready() && pool_tx.is_ready_for_pending_stage() {
                pool_tx.set_ready(true);
            }

            debug!(
                target: LOG_TARGET,
                "ON PROPOSE: process_newly_justified_block {} {} {}, QC[{}]",
                pool_tx.transaction_id(),
                pool_tx.current_stage(),
                local_committee_info.shard_group(),
                high_qc_id
            );

            change_set.set_next_transaction_update(pool_tx)?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn build_next_block(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        epoch: Epoch,
        next_height: NodeHeight,
        parent_block: LeafBlock,
        high_qc_certificate: QuorumCertificate,
        local_committee_info: &CommitteeInfo,
        local_claim_public_key_bytes: [u8; 32],
        dont_propose_transactions: bool,
        base_layer_block_height: u64,
        base_layer_block_hash: FixedHash,
        propose_epoch_end: bool,
    ) -> Result<NextBlock, HotStuffError> {
        // The parent block will only ever not exist if it is a dummy block
        let parent_exists = Block::record_exists(tx, parent_block.block_id())?;
        let start_of_chain_block = if parent_exists {
            // Parent exists - we can include its state in the MR calc, foreign propose etc
            parent_block
        } else {
            // Parent does not exist which means we have dummy blocks between the parent and the justified block so we
            // can exclude them from the query. There are a few queries that will fail if we used a non-existent block.
            high_qc_certificate.as_leaf_block()
        };

        let mut total_leader_fee = 0;

        let batch = if propose_epoch_end {
            ProposalBatch::default()
        } else {
            self.fetch_next_proposal_batch(
                tx,
                local_committee_info,
                dont_propose_transactions,
                start_of_chain_block,
            )?
        };

        debug!(target: LOG_TARGET, "🌿 PROPOSE: {batch}");

        let mut commands = if propose_epoch_end {
            BTreeSet::from_iter([Command::EndEpoch])
        } else {
            BTreeSet::from_iter(
                batch
                    .foreign_proposals
                    .iter()
                    .map(|fp| Command::ForeignProposal(fp.to_atom()))
                    .chain(
                        batch
                            .burnt_utxos
                            .iter()
                            .map(|bu| Command::MintConfidentialOutput(bu.to_atom())),
                    )
                    .chain(
                        batch
                            .evict_nodes
                            .into_iter()
                            .map(|public_key| Command::EvictNode(EvictNodeAtom { public_key })),
                    ),
            )
        };

        let mut change_set = ProposedBlockChangeSet::new(high_qc_certificate.as_leaf_block());

        // No need to include evidence from justified block if no transactions are included in the next block
        if !batch.transactions.is_empty() {
            // TODO(protocol-efficiency): We should process any foreign proposals included in this block to include
            // evidence. And that should determine if they are ready. However this is difficult because we
            // get the batch from the database which isnt aware of which foreign proposals we're going to
            // propose. This is why the system currently never proposes foreign proposals affecting a
            // transaction in the same block for LocalPrepare/LocalAccept and can result in evidence in the
            // atom having missing Prepare/Accept QCs (which are added on subsequent proposals).
            // let locked_block = LockedBlock::get(tx, epoch)?;
            // let num_proposals = batch.foreign_proposals.len();
            // let foreign_proposals = mem::replace(&mut batch.foreign_proposals, Vec::with_capacity(num_proposals));
            // for fp in foreign_proposals {
            //     if let Err(err) = process_foreign_block(
            //         tx,
            //         &high_qc_certificate.as_leaf_block(),
            //         &locked_block,
            //         &fp,
            //         local_committee_info,
            //         &mut change_set,
            //     ) {
            //         warn!(
            //             target: LOG_TARGET,
            //             "Failed to process foreign proposal: {}. Skipping this proposal...",
            //             err
            //         );
            //         // TODO: mark as invalid
            //         continue;
            //     }
            //     batch.foreign_proposals.push(fp);
            // }

            let justified_block = high_qc_certificate.get_block(tx)?;
            if !justified_block.is_justified() {
                // TODO: we dont need to process transactions here that are not in the batch
                self.process_newly_justified_block(
                    tx,
                    &justified_block,
                    *high_qc_certificate.id(),
                    local_committee_info,
                    &mut change_set,
                )?;
            }
        }

        // batch is empty for is_empty, is_epoch_end and is_epoch_start blocks
        let mut substate_store = PendingSubstateStore::new(
            tx,
            *start_of_chain_block.block_id(),
            self.config.consensus_constants.num_preshards,
        );
        let mut executed_transactions = HashMap::new();
        let timer = TraceTimer::info(LOG_TARGET, "Generating commands").with_iterations(batch.transactions.len());
        let mut lock_conflicts = TransactionLockConflicts::new();
        for mut transaction in batch.transactions {
            // Apply the transaction updates (if any) that occurred as a result of the justified block.
            // This allows us to propose evidence in the next block that relates to transactions in the justified block.
            change_set.apply_transaction_update(&mut transaction);
            if let Some(command) = self.transaction_pool_record_to_command(
                tx,
                &start_of_chain_block,
                transaction,
                local_committee_info,
                &mut substate_store,
                &mut executed_transactions,
                &mut lock_conflicts,
            )? {
                total_leader_fee += command
                    .committing()
                    .and_then(|tx| tx.leader_fee.as_ref())
                    .map(|f| f.fee)
                    .unwrap_or(0);
                // TODO: a BTreeSet changes the order from the original batch. Uncertain if this is a problem since the
                // proposer also processes transactions in the completed block order, however on_propose does perform
                // some operations (e.g. prepare, execute) in batch order. To ensure correctness, we should process
                // on_propose in canonical order.
                commands.insert(command);
            }
        }
        timer.done();

        // This relies on the UTXO commands being ordered after transaction commands
        for utxo in batch.burnt_utxos {
            let id = VersionedSubstateId::new(utxo.commitment, 0);
            let shard = id.to_shard(local_committee_info.num_preshards());
            let change = SubstateChange::Up {
                id,
                shard,
                // N/A
                transaction_id: Default::default(),
                substate: Substate::new(0, utxo.output),
            };

            substate_store.put(change)?;
        }

        debug!(
            target: LOG_TARGET,
            "command(s) for next block: [{}]",
            commands.display()
        );

        let timer = TraceTimer::info(LOG_TARGET, "Propose calculate state root");

        let pending_tree_diffs =
            PendingShardStateTreeDiff::get_all_up_to_commit_block(tx, start_of_chain_block.block_id())?;

        // Add proposer fee substate
        if total_leader_fee > 0 {
            let total_leader_fee_amt = total_leader_fee.try_into().map_err(|e| {
                HotStuffError::InvariantError(format!(
                    "Total leader fee ({total_leader_fee}) under/overflowed the Amount type: {e}"
                ))
            })?;

            // Apply leader fee to substate store before we calculate the state root
            apply_leader_fee_to_substate_store(
                &mut substate_store,
                local_claim_public_key_bytes,
                local_committee_info.shard_group().start(),
                local_committee_info.num_preshards(),
                total_leader_fee_amt,
            )?;
        }

        let (state_root, _) = calculate_state_merkle_root(
            tx,
            local_committee_info.shard_group(),
            pending_tree_diffs,
            substate_store.diff(),
        )?;
        timer.done();

        let non_local_shards = get_non_local_shards(substate_store.diff(), local_committee_info);

        let foreign_counters = ForeignSendCounters::get_or_default(tx, parent_block.block_id())?;
        let foreign_indexes = non_local_shards
            .iter()
            .map(|shard| (*shard, foreign_counters.get_count(*shard) + 1))
            .collect();

        let mut header = BlockHeader::create(
            self.config.network,
            *parent_block.block_id(),
            *high_qc_certificate.id(),
            next_height,
            epoch,
            local_committee_info.shard_group(),
            self.signing_service.public_key().clone(),
            state_root,
            &commands,
            total_leader_fee,
            foreign_indexes,
            None,
            EpochTime::now().as_u64(),
            base_layer_block_height,
            base_layer_block_hash,
            ExtraData::new(),
        )?;

        let signature = self.signing_service.sign(header.id());
        header.set_signature(signature);

        let next_block = Block::new(header, high_qc_certificate, commands);

        Ok(NextBlock {
            block: next_block,
            foreign_proposals: batch.foreign_proposals,
            executed_transactions,
            lock_conflicts,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn fetch_next_proposal_batch(
        &self,
        tx: &<<TConsensusSpec as ConsensusSpec>::StateStore as StateStore>::ReadTransaction<'_>,
        local_committee_info: &CommitteeInfo,
        dont_propose_transactions: bool,
        start_of_chain_block: LeafBlock,
    ) -> Result<ProposalBatch, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "fetch_next_proposal_batch");
        let foreign_proposals = ForeignProposal::get_all_new(
            tx,
            start_of_chain_block.block_id(),
            self.config.consensus_constants.max_block_size / 4,
        )?;

        if !foreign_proposals.is_empty() {
            debug!(
                target: LOG_TARGET,
                "🌿 Found {} foreign proposals for next block",
                foreign_proposals.len()
            );
        }

        let mut remaining_block_size = subtract_block_size_checked(
            Some(self.config.consensus_constants.max_block_size),
            foreign_proposals.len() * 4,
        );

        let burnt_utxos = remaining_block_size
            .map(|size| BurntUtxo::get_all_unproposed(tx, start_of_chain_block.block_id(), size))
            .transpose()?
            .unwrap_or_default();

        if !burnt_utxos.is_empty() {
            debug!(
                target: LOG_TARGET,
               "🌿 Found {} burnt utxos for next block",
                burnt_utxos.len()
            );
        }

        remaining_block_size = subtract_block_size_checked(remaining_block_size, burnt_utxos.len());

        let evict_nodes = remaining_block_size
            .map(|max| {
                let num_evicted =
                    ValidatorConsensusStats::count_number_evicted_nodes(tx, start_of_chain_block.epoch())?;
                let max_allowed_to_evict = u64::from(local_committee_info.max_failures())
                    .saturating_sub(num_evicted)
                    .min(max as u64);
                ValidatorConsensusStats::get_nodes_to_evict(
                    tx,
                    start_of_chain_block.block_id(),
                    self.config.consensus_constants.missed_proposal_evict_threshold,
                    max_allowed_to_evict,
                )
            })
            .transpose()?
            .unwrap_or_default();

        if !evict_nodes.is_empty() {
            debug!(
                target: LOG_TARGET,
                "🌿 Found {} EVICT nodes for next block",
                evict_nodes.len()
            )
        }

        remaining_block_size = subtract_block_size_checked(remaining_block_size, evict_nodes.len());

        let transactions = if dont_propose_transactions {
            vec![]
        } else {
            remaining_block_size
                .map(|size| {
                    self.transaction_pool
                        .get_batch_for_next_block(tx, size, start_of_chain_block.block_id())
                })
                .transpose()?
                .unwrap_or_default()
        };

        Ok(ProposalBatch {
            foreign_proposals,
            burnt_utxos,
            transactions,
            evict_nodes,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn prepare_transaction(
        &self,
        parent_block: &LeafBlock,
        tx_rec: &mut TransactionPoolRecord,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
        executed_transactions: &mut HashMap<TransactionId, TransactionExecution>,
        lock_conflicts: &mut TransactionLockConflicts,
    ) -> Result<Option<Command>, HotStuffError> {
        info!(
            target: LOG_TARGET,
            "👨‍🔧 PROPOSE: PREPARE transaction {}",
            tx_rec.transaction_id(),
        );

        let prepared = self
            .transaction_manager
            .prepare(
                substate_store,
                local_committee_info,
                parent_block.epoch(),
                tx_rec,
                parent_block.block_id(),
            )
            .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))?;

        if !prepared.is_involved(local_committee_info) {
            // CASE: execution was aborted and we're output-only
            warn!(
                target: LOG_TARGET,
                "❓️ Not involved in prepared transaction {}", tx_rec.transaction_id(),
            );
            // TODO: We may be the output for the transaction receipt, however we assume no outputs if aborting.
            //
            // return Ok(None);
        }

        if prepared.lock_status().is_any_failed() && !prepared.lock_status().is_hard_conflict() {
            warn!(
                target: LOG_TARGET,
                "⚠️ Transaction {} has lock conflicts, but no hard conflicts. Skipping proposing this transaction...",
                tx_rec.transaction_id(),
            );

            lock_conflicts.add(
                *tx_rec.transaction_id(),
                prepared.into_lock_status().into_lock_conflicts(),
            );
            return Ok(None);
        }

        let command = match prepared {
            PreparedTransaction::LocalOnly(local) => match *local {
                LocalPreparedTransaction::Accept { execution, .. } => {
                    tx_rec
                        .set_local_decision(execution.decision())
                        .set_transaction_fee(execution.transaction_fee())
                        .set_evidence(execution.to_evidence(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                        ));

                    info!(
                        target: LOG_TARGET,
                        "🏠️ Transaction {} is local only, proposing LocalOnly",
                        tx_rec.transaction_id(),
                    );

                    if tx_rec.current_decision().is_commit() {
                        let involved = NonZeroU64::new(1).expect("1 > 0");
                        let leader_fee =
                            tx_rec.calculate_leader_fee(involved, self.config.consensus_constants.fee_exhaust_divisor);
                        tx_rec.set_leader_fee(leader_fee);
                        let diff = execution.result().finalize.result.accept().ok_or_else(|| {
                            HotStuffError::InvariantError(format!(
                                "prepare_transaction: Transaction {} has COMMIT decision but execution failed when \
                                 proposing",
                                tx_rec.transaction_id(),
                            ))
                        })?;

                        if let Err(err) = substate_store.put_diff(*tx_rec.transaction_id(), diff) {
                            error!(
                                target: LOG_TARGET,
                                "🔒 Failed to write to temporary state store for transaction {} for LocalOnly: {}. Skipping proposing this transaction...",
                                tx_rec.transaction_id(),
                                err,
                            );
                            // Only error if it is not related to lock errors
                            let _err = err.ok_lock_failed()?;
                            return Ok(None);
                        }
                    }

                    executed_transactions.insert(*tx_rec.transaction_id(), execution);

                    let atom = tx_rec.get_current_transaction_atom();
                    Command::LocalOnly(atom)
                },
                LocalPreparedTransaction::EarlyAbort { execution } => {
                    info!(
                        target: LOG_TARGET,
                        "⚠️ Transaction is LOCAL-ONLY EARLY ABORT, proposing LocalOnly({}, ABORT)",
                        tx_rec.transaction_id(),
                    );
                    tx_rec
                        .set_local_decision(execution.decision())
                        .set_transaction_fee(execution.transaction_fee())
                        .set_evidence(execution.to_evidence(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                        ));
                    executed_transactions.insert(*tx_rec.transaction_id(), execution);
                    let atom = tx_rec.get_current_transaction_atom();
                    Command::LocalOnly(atom)
                },
            },

            PreparedTransaction::MultiShard(multishard) => {
                match multishard.into_evidence_or_execution() {
                    EvidenceOrExecution::Execution(execution) => {
                        // CASE: All inputs are local and outputs are foreign (i.e. the transaction is
                        // executed), or all inputs are foreign and this shard
                        // group is output only and, we've already received all pledges.
                        tx_rec.update_from_execution(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                            &execution,
                        );

                        // TODO: this is kinda hacky - we may not be involved in the transaction after ABORT execution,
                        // but this would be invalid so we ensure that we are added to evidence. Ideally, we wouldn't
                        // sequence this transaction at all - investigate.
                        tx_rec
                            .evidence_mut()
                            .add_shard_group(local_committee_info.shard_group());

                        if tx_rec.current_decision().is_commit() {
                            let involves_inputs = tx_rec.evidence().has_inputs(local_committee_info.shard_group());
                            if !involves_inputs {
                                let num_involved_shard_groups = tx_rec.evidence().num_shard_groups();
                                let involved = NonZeroU64::new(num_involved_shard_groups as u64).ok_or_else(|| {
                                    HotStuffError::InvariantError("Number of involved shard groups is 0".to_string())
                                })?;
                                let leader_fee = tx_rec.calculate_leader_fee(
                                    involved,
                                    self.config.consensus_constants.fee_exhaust_divisor,
                                );
                                tx_rec.set_leader_fee(leader_fee);
                            }
                        }

                        executed_transactions.insert(*tx_rec.transaction_id(), *execution);
                    },
                    EvidenceOrExecution::Evidence { evidence, .. } => {
                        // CASE: All local inputs were resolved. We need to continue with consensus to get the
                        // foreign inputs/outputs.
                        tx_rec.set_local_decision(Decision::Commit);
                        // Set partial evidence using local inputs and known outputs.
                        tx_rec.set_evidence(evidence);
                    },
                }

                info!(
                    target: LOG_TARGET,
                    "🌍 Transaction involves foreign shard groups, proposing Prepare({}, {})",
                    tx_rec.transaction_id(),
                    tx_rec.current_decision(),
                );

                let atom = tx_rec.get_local_transaction_atom();
                Command::Prepare(atom)
            },
        };

        Ok(Some(command))
    }

    fn all_or_some_prepare_transaction(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        parent_block: &LeafBlock,
        local_committee_info: &CommitteeInfo,
        tx_rec: &mut TransactionPoolRecord,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
        executed_transactions: &mut HashMap<TransactionId, TransactionExecution>,
    ) -> Result<Option<Command>, HotStuffError> {
        // Only set to abort if either the local or one or more foreign shards decided to ABORT
        if tx_rec.current_decision().is_abort() {
            return Ok(Some(Command::SomePrepare(tx_rec.get_current_transaction_atom())));
        }

        let transaction = TransactionRecord::get(tx, tx_rec.transaction_id())?;
        if !transaction.has_all_required_input_pledges(tx, local_committee_info)? {
            // TODO: investigate - this case does occur when all_input_shard_groups_prepared is used vs
            //       all_shard_groups_prepared in can_continue_to, not sure why.
            // Once case where this can happen if we received a LocalAccept pledge, which will skip sending the substate
            // values, but not LocalPrepare (which contains substate values). This could be solved by
            // (re-)requesting the LocalPrepare pledge.
            error!(
                target: LOG_TARGET,
                "BUG: attempted to propose transaction {} as AllPrepared but not all input pledges were found. This transaction should not have been marked as ready.",
                tx_rec.transaction_id(),
            );
            return Ok(None);
        }
        let mut execution = self.execute_transaction(tx, &parent_block.block_id, parent_block.epoch, transaction)?;

        // Try to lock all local outputs
        let local_outputs = execution
            .resulting_outputs()
            .iter()
            .filter(|o| local_committee_info.includes_substate_id(o.substate_id()));
        let lock_status = substate_store.try_lock_all(*tx_rec.transaction_id(), local_outputs, false)?;
        if let Some(err) = lock_status.failures().first() {
            warn!(
                target: LOG_TARGET,
                "⚠️ Failed to lock outputs for transaction {}: {}",
                tx_rec.transaction_id(),
                err,
            );
            // If the transaction does not lock, we propose to abort it
            execution.set_abort_reason(RejectReason::FailedToLockOutputs(err.to_string()));
            tx_rec.update_from_execution(
                local_committee_info.num_preshards(),
                local_committee_info.num_committees(),
                &execution,
            );

            executed_transactions.insert(*tx_rec.transaction_id(), execution);
            return Ok(Some(Command::AllPrepare(tx_rec.get_current_transaction_atom())));
        }

        tx_rec.update_from_execution(
            local_committee_info.num_preshards(),
            local_committee_info.num_committees(),
            &execution,
        );

        executed_transactions.insert(*tx_rec.transaction_id(), execution);
        // If we locally decided to ABORT, we are still saying that we think all prepared and, after execution decide to
        // ABORT. When we enter the acceptance phase, we will propose SomeAccept for this case.
        Ok(Some(Command::AllPrepare(tx_rec.get_current_transaction_atom())))
    }

    fn accept_transaction(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        parent_block: &LeafBlock,
        tx_rec: &mut TransactionPoolRecord,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
    ) -> Result<Option<Command>, HotStuffError> {
        if tx_rec.current_decision().is_abort() {
            return Ok(Some(Command::SomeAccept(tx_rec.get_current_transaction_atom())));
        }

        let execution =
            BlockTransactionExecution::get_pending_for_block(tx, tx_rec.transaction_id(), &parent_block.block_id)
                .optional()?
                .ok_or_else(|| {
                    HotStuffError::InvariantError(format!(
                        "accept_transaction: Transaction {} has COMMIT decision but execution is missing",
                        tx_rec.transaction_id(),
                    ))
                })?;
        let diff = execution.result().finalize.accept().ok_or_else(|| {
            HotStuffError::InvariantError(format!(
                "local_accept_transaction: Transaction {} has COMMIT decision but execution failed when proposing",
                tx_rec.transaction_id(),
            ))
        })?;
        substate_store.put_diff(
            *tx_rec.transaction_id(),
            &filter_diff_for_committee(local_committee_info, diff),
        )?;
        let atom = self.get_transaction_atom_with_leader_fee(tx_rec)?;
        Ok(Some(Command::AllAccept(atom)))
    }

    fn get_transaction_atom_with_leader_fee(
        &self,
        tx_rec: &mut TransactionPoolRecord,
    ) -> Result<TransactionAtom, HotStuffError> {
        if tx_rec.current_decision().is_commit() {
            let num_involved_shard_groups = tx_rec.evidence().num_shard_groups();
            let involved = NonZeroU64::new(num_involved_shard_groups as u64).ok_or_else(|| {
                HotStuffError::InvariantError(format!(
                    "PROPOSE: Transaction {} involves zero shard groups",
                    tx_rec.transaction_id(),
                ))
            })?;
            let leader_fee = tx_rec.calculate_leader_fee(involved, self.config.consensus_constants.fee_exhaust_divisor);
            tx_rec.set_leader_fee(leader_fee);
        }
        let atom = tx_rec.get_current_transaction_atom();
        Ok(atom)
    }

    fn execute_transaction(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        parent_block_id: &BlockId,
        current_epoch: Epoch,
        transaction: TransactionRecord,
    ) -> Result<TransactionExecution, HotStuffError> {
        // Might have been executed already if all inputs are local
        if let Some(execution) =
            BlockTransactionExecution::get_pending_for_block(tx, transaction.id(), parent_block_id).optional()?
        {
            info!(
                target: LOG_TARGET,
                "👨‍🔧 PROPOSE: Using existing transaction execution {} ({})",
                transaction.id(), execution.execution.decision(),
            );
            return Ok(execution.into_transaction_execution());
        }

        let pledged = PledgedTransaction::load_pledges(tx, transaction)?;

        info!(
            target: LOG_TARGET,
            "👨‍🔧 PROPOSE: Executing transaction {} (pledges: {} local, {} foreign)",
            pledged.id(), pledged.local_pledges.len(), pledged.foreign_pledges.len(),
        );

        let executed = self
            .transaction_manager
            .execute(current_epoch, pledged)
            .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))?;

        Ok(executed.into_execution())
    }
}

pub fn get_non_local_shards(diff: &[SubstateChange], local_committee_info: &CommitteeInfo) -> HashSet<Shard> {
    diff.iter()
        .map(|ch| {
            ch.versioned_substate_id()
                .to_shard(local_committee_info.num_preshards())
        })
        .filter(|shard| local_committee_info.shard_group().contains(shard))
        .collect()
}

#[derive(Default)]
struct ProposalBatch {
    pub foreign_proposals: Vec<ForeignProposal>,
    pub burnt_utxos: Vec<BurntUtxo>,
    pub transactions: Vec<TransactionPoolRecord>,
    pub evict_nodes: Vec<PublicKey>,
}

impl Display for ProposalBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} transaction(s), {} foreign proposal(s), {} UTXOs, {} evict",
            self.transactions.len(),
            self.foreign_proposals.len(),
            self.burnt_utxos.len(),
            self.evict_nodes.len()
        )
    }
}

fn subtract_block_size_checked(remaining_block_size: Option<usize>, by: usize) -> Option<usize> {
    remaining_block_size
        .and_then(|sz| sz.checked_sub(by))
        .filter(|sz| *sz > 0)
}
