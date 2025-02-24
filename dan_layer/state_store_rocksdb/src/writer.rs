//  Copyright 2024. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{iter::Peekable, ops::Deref, sync::{Arc, Mutex}, time::{SystemTime, UNIX_EPOCH}};

use indexmap::IndexMap;
use log::*;
use rocksdb::{Transaction, TransactionDB};
use tari_dan_common_types::{
    optional::Optional,
    shard::Shard,
    Epoch,
    NodeAddressable,
    NodeHeight,
    ShardGroup,
    SubstateLockType,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_dan_storage::{
    consensus_models::{
        Block, BlockId, BlockTransactionExecution, BurntUtxo, Decision, EpochCheckpoint, Evidence, ForeignParkedProposal, ForeignProposal, ForeignProposalStatus, ForeignReceiveCounters, ForeignSendCounters, HighQc, LastExecuted, LastProposed, LastSentVote, LastVoted, LeafBlock, LockConflict, LockedBlock, NoVoteReason, PendingShardStateTreeDiff, QcId, QuorumCertificate, StateTransition, StateTransitionId, SubstateChange, SubstateCreatedProof, SubstateData, SubstateDestroyed, SubstateDestroyedProof, SubstateLock, SubstatePledge, SubstatePledges, SubstateRecord, SubstateUpdate, TransactionPool, TransactionPoolConfirmedStage, TransactionPoolRecord, TransactionPoolStage, TransactionPoolStatusUpdate, TransactionRecord, VersionedStateHashTreeDiff, Vote
    }, Ordering, StateStoreReadTransaction, StateStoreWriteTransaction, StorageError
};
use tari_engine_types::{substate::SubstateId, template_models::UnclaimedConfidentialOutputAddress};
use tari_state_tree::{Node, NodeKey, StaleTreeNode, TreeNode, Version};
use tari_transaction::TransactionId;
use tari_utilities::ByteArray;
use time::{OffsetDateTime, PrimitiveDateTime};
use tari_common_types::types::PublicKey;
use tari_dan_storage::consensus_models::ValidatorStatsUpdate;

use crate::{error::RocksDbStorageError, model::{block::BlockModel, block_diff::{BlockDiffData, BlockDiffModel}, block_transaction_execution::{BlockTransactionExecutionModel, BlockTransactionExecutionModelData}, epoch_checkpoint::EpochCheckpointModel, foreign_parked_blocks::ForeignParkedBlockModel, foreign_proposal::ForeignProposalModel, foreign_receive_counter::ForeignReceiveCounterModel, foreign_send_counter::{ForeignSendCounterData, ForeignSendCounterModel}, foreign_substate_pledge::{ForeignSubstatePledgeData, ForeignSubstatePledgeModel}, high_qc::HighQcModel, last_executed::LastExecutedModel, last_proposed::LastProposedModel, last_sent_vote::LastSentVoteModel, last_voted::LastVotedModel, leaf_block::LeafBlockModel, locked_block::LockedBlockModel, missing_transactions::{MissingTransaction, MissingTransactionModel}, model::{ModelColumnFamily, RocksdbModel}, parked_block::{ParkedBlockData, ParkedBlockModel}, pending_state_tree_diff::{PendingStateTreeDiffData, PendingStateTreeDiffModel}, quorum_certificate::QuorumCertificateModel, state_transition::{StateTransitionModel, StateTransitionModelData}, state_tree::{StateTreeModel, StateTreeModelData}, state_tree_shard_versions::{StateTreeShardVersionModel, StateTreeShardVersionModelData}, substate::SubstateModel, transaction::TransactionModel, transaction_pool::TransactionPoolModel, transaction_pool_state_update::{TransactionPoolStateUpdateModel, TransactionPoolStateUpdateModelData}, vote::VoteModel}, reader::RocksDbStateStoreReadTransaction, utils::{RocksdbSeq, RocksdbTimestamp}};

use bincode;

const LOG_TARGET: &str = "tari::dan::storage::state_store_rocksdb::writer";

pub struct RocksDbStateStoreWriteTransaction<'a, TAddr> {
    /// None indicates if the transaction has been explicitly committed/rolled back
    transaction: Option<RocksDbStateStoreReadTransaction<'a, TAddr>>,
    db: Arc<TransactionDB>,
}

impl<'a, TAddr: NodeAddressable> RocksDbStateStoreWriteTransaction<'a, TAddr> {
    pub fn new(db: Arc<TransactionDB>, tx: Transaction<'a, TransactionDB>) -> Self {
        Self {
            db: db.clone(),
            transaction: Some(RocksDbStateStoreReadTransaction::new(db, tx)),
        }
    }

    fn parked_blocks_insert(
        &mut self,
        block: &Block,
        foreign_proposals: &[ForeignProposal],
    ) -> Result<(), StorageError> {
        let operation = "parked_blocks_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        
        // check if block exists in blocks model
        let block_id = block.id();
        let key = BlockModel::key_from_block_id(block_id);
        let block_exists = BlockModel::key_exists(tx, operation, &key)?;
        if block_exists {
            return Err(StorageError::QueryError {
                reason: format!("Cannot park block {block_id} that already exists in blocks table"),
            });
        }

        // check if block already exists in parked_blocks
        let key = ParkedBlockModel::key_from_block_id(block_id);
        let already_parked = ParkedBlockModel::key_exists(tx, operation, &key)?;
        if already_parked {
            return Ok(());
        }

        let parked_block_data = ParkedBlockData {
            block: block.clone(),
            foreign_proposals: foreign_proposals.to_vec()
        };
        ParkedBlockModel::put(self.db.clone(), tx, operation, &parked_block_data)?;

        Ok(())
    }

    fn parked_blocks_remove(&mut self, block_id: &str) -> Result<(Block, Vec<ForeignProposal>), StorageError> {
        let operation = "parked_blocks_remove";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key = ParkedBlockModel::key_from_block_id_str(block_id);
        let data = ParkedBlockModel::get(tx, operation, &key)?;

        ParkedBlockModel::delete(self.db.clone(), tx, operation, &key)?;

        Ok((data.block, data.foreign_proposals))
    }
}

impl<'tx, TAddr: NodeAddressable + 'tx> StateStoreWriteTransaction for RocksDbStateStoreWriteTransaction<'tx, TAddr> {
    type Addr = TAddr;

    fn commit(&mut self) -> Result<(), StorageError> {
        // Take so that we mark this transaction as complete in the drop impl
        self.transaction.take().unwrap().commit()?;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), StorageError> {
        // Take so that we mark this transaction as complete in the drop impl
        self.transaction.take().unwrap().rollback()?;
        Ok(())
    }

    fn blocks_insert(&mut self, block: &Block) -> Result<(), StorageError> {
        let now: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| StorageError::General { details: e.to_string() })?
            .as_millis()
            .try_into()
            .unwrap();
        let block_time= Some(now - block.timestamp());
        let mut block = block.clone();
        block.set_block_time(block_time);
        
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        Ok(BlockModel::put(self.db.clone(), tx, "blocks_insert", &block)?)
    }

    fn blocks_delete(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        let operation = "blocks_delete";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        let key = BlockModel::key_from_block_id(block_id);
        BlockModel::delete(self.db.clone(), tx, operation, &key)?;

        // NOTE: we not implementing the equivalent of the sqlite "diagnostic_deleted_blocks" table as it does not seem to be used

        Ok(())
    }

    fn blocks_set_flags(
        &mut self,
        block_id: &BlockId,
        is_committed: Option<bool>,
        is_justified: Option<bool>,
    ) -> Result<(), StorageError> {
        let operation = "blocks_set_flags";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // fetch the related block
        let key: String = BlockModel::key_from_block_id(block_id);
        let mut block = BlockModel::get(tx, operation, &key)?;

        // set the flags
        is_committed.map(|value| block.set_is_committed(value));
        is_justified.map(|value| block.set_is_justified(value));
        
        // update the block in rocksDb
        // TODO: is it better to use a RocksDB merge operator?
        BlockModel::put(self.db.clone(), tx, operation, &block)?;

        Ok(())
    }

    fn block_diffs_insert(&mut self, block_id: &BlockId, changes: &[SubstateChange]) -> Result<(), StorageError> {
        let operation = "block_diffs_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // TODO: use batch insertion in rocksdb
        for change in changes {
            let block_diff_data = BlockDiffData {
                block_id: *block_id,
                substate_id: change.versioned_substate_id().substate_id.clone(),
                change: change.clone(),
                created_at: RocksdbTimestamp::now(),
            };

            BlockDiffModel::put(self.db.clone(), tx, operation, &block_diff_data)?;
        }

        Ok(())
    }

    fn block_diffs_remove(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        let operation = "block_diffs_remove";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key_prefix = BlockDiffModel::build_key_prefix(*block_id, None);
        let values = BlockDiffModel::multi_get(tx, Some(&key_prefix), Ordering::Ascending)?;
        for value in values {
            let key = BlockDiffModel::key(&value);
            BlockDiffModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        Ok(())
    }

    fn quorum_certificates_insert(&mut self, qc: &QuorumCertificate) -> Result<(), StorageError> {
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        Ok(QuorumCertificateModel::put(self.db.clone(), tx, "quorum_certificates_insert", &qc)?)
    }

    fn quorum_certificates_set_shares_processed(&mut self, qc_id: &QcId) -> Result<(), StorageError> {
        let operation = "quorum_certificates_set_shares_processed";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // fetch the qc
        let key: String = QuorumCertificateModel::key_from_qc_id(qc_id);
        let mut qc = QuorumCertificateModel::get(tx, operation, &key)?;

        // set the value
        qc.set_is_shares_processed(true);
        
        // update the block in rocksDb
        QuorumCertificateModel::put(self.db.clone(), tx, operation, &qc)?;

        Ok(())
    }

    fn last_sent_vote_set(&mut self, last_sent_vote: &LastSentVote) -> Result<(), StorageError> {
        let operation = "last_sent_vote_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        LastSentVoteModel::put(self.db.clone(), tx, operation, last_sent_vote)?;

        Ok(())
    }

    fn last_voted_set(&mut self, last_voted: &LastVoted) -> Result<(), StorageError> {
        let operation = "last_voted_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        LastVotedModel::put(self.db.clone(), tx, operation, &last_voted.into())?;

        Ok(())
    }

    fn last_votes_unset(&mut self, last_voted: &LastVoted) -> Result<(), StorageError> {
        let operation = "last_votes_unset";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key = LastVotedModel::key(&last_voted.into());
        LastVotedModel::delete(self.db.clone(), tx, operation, &key)?;

        Ok(())
    }

    fn last_executed_set(&mut self, last_exec: &LastExecuted) -> Result<(), StorageError> {
        let operation = "last_executed_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        LastExecutedModel::put(self.db.clone(), tx, operation, &last_exec)?;

        Ok(())
    }

    fn last_proposed_set(&mut self, last_proposed: &LastProposed) -> Result<(), StorageError> {
        let operation = "last_proposed_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        LastProposedModel::put(self.db.clone(), tx, operation, &last_proposed.into())?;

        Ok(())
    }

    fn last_proposed_unset(&mut self, last_proposed: &LastProposed) -> Result<(), StorageError> {
        let operation = "last_proposed_unset";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key = LastProposedModel::key(&last_proposed.into());
        LastProposedModel::delete(self.db.clone(), tx, operation, &key)?;

        Ok(())
    }

    fn leaf_block_set(&mut self, leaf_node: &LeafBlock) -> Result<(), StorageError> {
        let operation = "leaf_block_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        LeafBlockModel::put(self.db.clone(), tx, operation, &leaf_node.into())?;

        Ok(())
    }

    fn locked_block_set(&mut self, locked_block: &LockedBlock) -> Result<(), StorageError> {
        let operation = "locked_block_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        LockedBlockModel::put(self.db.clone(), tx, operation, &locked_block.into())?;

        Ok(())
    }

    fn high_qc_set(&mut self, high_qc: &HighQc) -> Result<(), StorageError> {
        let operation = "high_qc_set";
        let tx: &mut Transaction<'_, TransactionDB> = self.transaction.as_mut().unwrap().rocksdb_transaction();
        HighQcModel::put(self.db.clone(), tx, operation, &high_qc.into())?;

        Ok(())
    }

    fn foreign_proposals_upsert(
        &mut self,
        foreign_proposal: &ForeignProposal,
        proposed_in_block: Option<BlockId>,
    ) -> Result<(), StorageError> {
        let operation = "foreign_proposals_upsert";
        let tx: &mut Transaction<'_, TransactionDB> = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key = ForeignProposalModel::key(foreign_proposal);
        let key_exists = ForeignProposalModel::key_exists(tx, operation, &key)?;
        if !key_exists {
            ForeignProposalModel::put(self.db.clone(), tx, operation, &foreign_proposal)?;
        }

        let block = foreign_proposal.block();
        if let Some(proposed_in_block) = proposed_in_block {
            self.foreign_proposals_set_proposed_in(block.id(), &proposed_in_block)?;
        }

        Ok(())
    }

    fn foreign_proposals_delete(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        let operation = "foreign_proposals_delete";
        let tx: &mut Transaction<'_, TransactionDB> = self.transaction.as_mut().unwrap().rocksdb_transaction();
        let key = ForeignProposalModel::key_from_block_id(block_id);
        ForeignProposalModel::delete(self.db.clone(), tx, operation, &key)?;

        Ok(())
    }

    fn foreign_proposals_delete_in_epoch(&mut self, epoch: Epoch) -> Result<(), StorageError> {
        let operation = "foreign_proposals_delete_in_epoch";
        let tx: &mut Transaction<'_, TransactionDB> = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // get all the proposals for the epoch
        type Cf = crate::model::foreign_proposal::EpochStatusColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::key_prefix_from_epoch(&epoch);
        let proposals = ForeignProposalModel::multi_get_cf(self.db.clone(), tx, operation, cf, &key_prefix, Ordering::Ascending)?;

        // delete all the epoch proposals in db
        for proposal in proposals {
            let key = ForeignProposalModel::key(&proposal);
            ForeignProposalModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        Ok(())
    }

    fn foreign_proposals_set_status(
        &mut self,
        block_id: &BlockId,
        status: ForeignProposalStatus,
    ) -> Result<(), StorageError> {
        let operation = "foreign_proposals_set_status";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // fetch the proposal
        let key: String = ForeignProposalModel::key_from_block_id(block_id);
        let proposal = ForeignProposalModel::get(tx, operation, &key)?;

        // set the value
        let updated_proposal = ForeignProposal {
            block: proposal.block,
            block_pledge: proposal.block_pledge,
            justify_qc: proposal.justify_qc,
            proposed_by_block: proposal.proposed_by_block,
            status,
        };
        
        // update the block in rocksDb
        ForeignProposalModel::put(self.db.clone(), tx, operation, &updated_proposal)?;

        Ok(())
    }

    fn foreign_proposals_set_proposed_in(
        &mut self,
        block_id: &BlockId,
        proposed_in_block: &BlockId,
    ) -> Result<(), StorageError> {
        let operation = "foreign_proposals_set_proposed_in";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // fetch the proposal
        let key: String = ForeignProposalModel::key_from_block_id(block_id);
        let proposal = ForeignProposalModel::get(tx, operation, &key)?;

        // set the value
        let updated_proposal = ForeignProposal {
            block: proposal.block,
            block_pledge: proposal.block_pledge,
            justify_qc: proposal.justify_qc,
            proposed_by_block: Some(*proposed_in_block),
            status: ForeignProposalStatus::Proposed,
        };
        
        // update the block in rocksDb
        ForeignProposalModel::put(self.db.clone(), tx, operation, &updated_proposal)?;

        Ok(())
    }

    fn foreign_proposals_clear_proposed_in(&mut self, proposed_in_block: &BlockId) -> Result<(), StorageError> {
        let operation = "foreign_proposals_clear_proposed_in";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // get the proposal based on the "proposed_in_block" field
        type Cf = crate::model::foreign_proposal::ProposedColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::key_prefix_from_proposed_by_block(&proposed_in_block);
        let proposal = ForeignProposalModel::get_cf(self.db.clone(), tx, operation, cf, Some(&key_prefix), Ordering::Ascending)?
            .ok_or_else(|| StorageError::NotFound {
                item: "foreign_proposals",
                key: proposed_in_block.to_string(),
            })?;

        // set the values
        let updated_proposal = ForeignProposal {
            block: proposal.block,
            block_pledge: proposal.block_pledge,
            justify_qc: proposal.justify_qc,
            proposed_by_block: None,
            status: ForeignProposalStatus::New,
        };
        
        // update the block in rocksDb
        ForeignProposalModel::put(self.db.clone(), tx, operation, &updated_proposal)?;

        Ok(())
    }

    fn foreign_send_counters_set(
        &mut self,
        foreign_send_counter: &ForeignSendCounters,
        block_id: &BlockId,
    ) -> Result<(), StorageError> {
        let operation = "foreign_send_counters_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();   

        let value = ForeignSendCounterData::new(*block_id, foreign_send_counter.clone());

        ForeignSendCounterModel::put(self.db.clone(), tx, operation, &value)?;

        Ok(())
    }

    fn foreign_receive_counters_set(
        &mut self,
        foreign_receive_counter: &ForeignReceiveCounters,
    ) -> Result<(), StorageError> {
        let operation = "foreign_receive_counters_set";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();   

        ForeignReceiveCounterModel::put(self.db.clone(), tx, operation, &foreign_receive_counter.into())?;

        Ok(())
    }

    fn transactions_insert(&mut self, tx_rec: &TransactionRecord) -> Result<(), StorageError> {
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        TransactionModel::put(self.db.clone(), tx, "transactions_insert", &tx_rec)?;
        Ok(())
    }

    fn transactions_update(&mut self, transaction_rec: &TransactionRecord) -> Result<(), StorageError> {
        let operation = "transactions_update";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key = TransactionModel::key_from_transaction_id(transaction_rec.id());
        if !TransactionModel::key_exists(tx, operation, &key)? {
            return Err(StorageError::NotFound {
                item: "transaction",
                key: transaction_rec.id().to_string(),
            });
        }

        // update the transaction in rocksDb
        // TODO: is it better to use a RocksDB merge operator?
        TransactionModel::put(self.db.clone(), tx, operation, &transaction_rec)?;

        Ok(())
    }

    fn transactions_save_all<'a, I: IntoIterator<Item = &'a TransactionRecord>>(
        &mut self,
        txs: I,
    ) -> Result<(), StorageError> {
        let operation = "transactions_save_all";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        for transaction in txs {
            TransactionModel::put(self.db.clone(), tx, operation, transaction)?;
        }

        Ok(())
    }

    fn transactions_finalize_all<'a, I: IntoIterator<Item = &'a TransactionPoolRecord>>(
        &mut self,
        block_id: BlockId,
        transactions: I,
    ) -> Result<(), StorageError> {
        let operation = "transactions_finalize_all";

        if !self.blocks_exists(&block_id)? {
            return Err(StorageError::QueryError {
                reason: format!(
                    "{}: Cannot finalize transactions for non-existent block {}",
                    operation,
                    block_id
                ),
            });
        }

        let mut updated_recs = vec![];
        for rec in transactions {
            let exec = self
                    .transaction_executions_get_pending_for_block(rec.transaction_id(), &block_id)
                    .optional()?
                    .ok_or_else(|| StorageError::DataInconsistency {
                        details: format!(
                            "transactions_finalize_all: No pending execution for transaction {}",
                            rec.transaction_id()
                        ),
                    })?;
            let mut db_rec = self.transactions_get(rec.transaction_id())?;

            db_rec.resolved_inputs = Some(exec.resolved_inputs().to_vec());
            db_rec.resulting_outputs = Some(exec.resulting_outputs().to_vec());
            db_rec.execution_result = Some(exec.result().clone());
            db_rec.final_decision = Some(db_rec.current_decision());
            db_rec.abort_reason = exec.abort_reason().cloned();
            
            updated_recs.push(db_rec);
        }

        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        for rec in updated_recs {
            TransactionModel::put(self.db.clone(), tx, operation, &rec)?;
        }

        Ok(())
    }

    fn transaction_executions_insert_or_ignore(
        &mut self,
        transaction_execution: &BlockTransactionExecution,
    ) -> Result<bool, StorageError> {
        let operation = "transaction_executions_insert_or_ignore";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let value = BlockTransactionExecutionModelData::from(transaction_execution);
        BlockTransactionExecutionModel::put(self.db.clone(), tx, operation, &value)?;

        return Ok(true)
    }

    fn transaction_executions_remove_any_by_block_id(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        let operation = "transaction_executions_remove_any_by_block_id";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        type Cf = crate::model::block_transaction_execution::BlockColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::key_prefix_by_block(block_id);
        let ordering = Ordering::Ascending;
        let execs = BlockTransactionExecutionModel::multi_get_cf(self.db.clone(), tx, operation, cf, &key_prefix, ordering)?;

        for exec in execs {
            let key = BlockTransactionExecutionModel::key(&exec);
            BlockTransactionExecutionModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        Ok(())
    }

    fn transaction_pool_insert_new(
        &mut self,
        tx_id: TransactionId,
        decision: Decision,
        initial_evidence: &Evidence,
        is_ready: bool,
        is_global: bool,
    ) -> Result<(), StorageError> {
        let value = TransactionPoolRecord::load(
            tx_id,
            initial_evidence.clone(),
            is_global,
            0,
            None,
            TransactionPoolStage::New,
             None,
            decision,
            None,
            None,
            is_ready,
        );

        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        Ok(TransactionPoolModel::put(self.db.clone(), tx, "transaction_pool_insert_new", &value)?)
    }

    fn transaction_pool_add_pending_update(
        &mut self,
        block_id: &BlockId,
        update: &TransactionPoolStatusUpdate,
    ) -> Result<(), StorageError> {
        let operation = "transaction_pool_add_pending_update";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // fetch the related block
        let key: String = BlockModel::key_from_block_id(block_id);
        let block = BlockModel::get(tx, operation, &key)?;

        // insert the update
        let value = TransactionPoolStateUpdateModelData {
            block_id: *block_id,
            block_height: block.height(),
            is_applied: false,
            transaction_id: *update.transaction_id(),
            evidence: update.evidence().clone(),
            transaction_fee: update.transaction_fee(),
            leader_fee: update.leader_fee().cloned(),
            stage: update.stage(),
            local_decision: update.decision(),
            remote_decision: update.remote_decision(),
            is_ready: update.is_ready(),
        };
        TransactionPoolStateUpdateModel::put(self.db.clone(), tx, operation, &value)?;

        // Set is_ready and pending_stage to the updated values. This allows has_uncommitted_transactions to return an
        // accurate value without querying records in the updates table.
        // TODO: is it better to use a RocksDB merge operator?
        let transaction_id = update.transaction().transaction_id();
        let key = TransactionPoolModel::key_from_transaction_id(&transaction_id);
        let mut transaction_pool_value = TransactionPoolModel::get(tx, operation, &key)?;
        transaction_pool_value.set_is_ready(update.is_ready_now());
        transaction_pool_value.set_pending_stage(Some(update.stage()));
        TransactionPoolModel::put(self.db.clone(), tx, operation, &transaction_pool_value)?;

        Ok(())
    }

    fn transaction_pool_remove(&mut self, _transaction_id: &TransactionId) -> Result<(), StorageError> {
        // This methdod is not used
        todo!()
    }

    fn transaction_pool_remove_all<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<Vec<TransactionPoolRecord>, StorageError> {
        let operation = "transaction_pool_remove_all";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        let transaction_ids = transaction_ids.into_iter().collect::<Vec<_>>();

        let mut transactions = vec![];
        for transaction_id in &transaction_ids {
            let key = TransactionPoolModel::key_from_transaction_id(transaction_id);
            let transaction = TransactionPoolModel::get(tx, operation, &key)?;
            transactions.push(transaction);
            TransactionPoolModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        if transactions.len() != transaction_ids.len() {
            return Err(RocksDbStorageError::NotAllTransactionsFound {
                operation: "transaction_pool_remove_all",
                details: format!(
                    "Found {} transactions, but {} were queried",
                    transactions.len(),
                    transaction_ids.len()
                ),
            }
            .into());
        }
            
        Ok(transactions)
    }

    fn transaction_pool_confirm_all_transitions(&mut self, new_locked_block: &LockedBlock) -> Result<(), StorageError> {
        let operation = "transaction_pool_confirm_all_transitions";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // fetch all the transaction updates that are not applied yet for the new block 
        let key_prefix = TransactionPoolStateUpdateModel::key_prefix_by_block_id(new_locked_block.block_id());
        let mut updates: Vec<TransactionPoolStateUpdateModelData> = TransactionPoolStateUpdateModel::multi_get(tx, Some(&key_prefix), Ordering::Ascending)?
            // TODO: do the filtering at the rocksdb query (use a dedicated column family?)
            .into_iter()
            .filter(|u| {
                u.block_height <= new_locked_block.height() &&
                u.is_applied == false 
            })
            .collect();

        debug!(
            target: LOG_TARGET,
            "transaction_pool_confirm_all_transitions: new_locked_block={}, {} updates",  new_locked_block, updates.len()
        );

        // mark all transaction updates as applied
        for mut update in &mut updates {
            update.is_applied = true;
            TransactionPoolStateUpdateModel::put(self.db.clone(), tx, operation, &update)?;
        }

        // update the transactions in the transaction pool
        for update in &updates {
            let confirm_stage = match update.stage {
                TransactionPoolStage::LocalPrepared => Some(Some(TransactionPoolConfirmedStage::ConfirmedPrepared)),
                TransactionPoolStage::LocalAccepted => Some(Some(TransactionPoolConfirmedStage::ConfirmedAccepted)),
                _ => None,
            };

            // TODO: use instead the rocksdb "merge" operator for better performance?
            let key = TransactionPoolModel::key_from_transaction_id(&update.transaction_id);
            let mut tx_pool_value = TransactionPoolModel::get(tx, operation, &key)?;
            tx_pool_value.set_stage(update.stage);
            tx_pool_value.set_local_decision(update.local_decision);
            tx_pool_value.set_transaction_fee(update.transaction_fee);
            if let Some(leader_fee) = &update.leader_fee {
                tx_pool_value.set_leader_fee(leader_fee.clone());
            }
            tx_pool_value.set_evidence(update.evidence.clone());
            tx_pool_value.set_is_ready(update.is_ready);
            if let Some(remote_decision) = update.remote_decision {
                tx_pool_value.set_remote_decision(remote_decision);
            }
            // TODO: tx_pool_value.set_confirm_stage?

            TransactionPoolModel::put(self.db.clone(), tx, operation, &tx_pool_value)?;
        }

        Ok(())
    }

    fn transaction_pool_state_updates_remove_any_by_block_id(
        &mut self,
        block_id: &BlockId,
    ) -> Result<(), StorageError> {
        let operation = "transaction_pool_state_updates_remove_any_by_block_id";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key_prefix = TransactionPoolStateUpdateModel::key_prefix_by_block_id(block_id);
        let updates = TransactionPoolStateUpdateModel::multi_get(tx, Some(&key_prefix), Ordering::Ascending)?;

        for update in updates {
            let key = TransactionPoolStateUpdateModel::key(&update);
            TransactionPoolStateUpdateModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        Ok(())
    }

    fn missing_transactions_insert<'a, IMissing: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        block: &Block,
        foreign_proposals: &[ForeignProposal],
        missing_transaction_ids: IMissing,
    ) -> Result<(), StorageError> {
        {
            self.parked_blocks_insert(block, foreign_proposals)?;
        }

        let operation = "missing_transactions_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        for transaction_id in missing_transaction_ids {
            let value = MissingTransaction {
                block_id: *block.id(),
                block_height: RocksdbSeq(block.height().as_u64()),
                transaction_id: *transaction_id,
            };
            MissingTransactionModel::put(self.db.clone(), tx, operation, &value)?;
        }

        Ok(())
    }

    fn missing_transactions_remove(
        &mut self,
        current_height: NodeHeight,
        transaction_id: &TransactionId,
    ) -> Result<Option<(Block, Vec<ForeignProposal>)>, StorageError> {
        let operation = "missing_transactions_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // get the block id of the transaction
        let height = RocksdbSeq(current_height.as_u64());
        let key = MissingTransactionModel::key_prefix_from_transaction_and_height(&*transaction_id, Some(height));
        if !MissingTransactionModel::key_exists(tx, operation, &key)? {
            return Ok(None);
        }
        let value = MissingTransactionModel::get(tx, operation, &key)?;
        let block_id = value.block_id;

        // delete the missing transaction
        MissingTransactionModel::delete(self.db.clone(), tx, operation, &key)?;

        // if the block has no more missing transactions, delete all missing transactions from previous blocks
        type BlockIdCf = crate::model::missing_transactions::BlockIdColumnFamily;
        let key_prefix = BlockIdCf::build_key_prefix_by_block(&block_id);
        let num_remaining = MissingTransactionModel::count_cf(self.db.clone(), tx, BlockIdCf::name(), Some(&key_prefix))?;

        if num_remaining == 0 {
            // delete all entries that are for previous heights
            type BlockHeightCf = crate::model::missing_transactions::BlockHeightColumnFamily;
            let key_prefix = MissingTransactionModel::key_prefix();
            let values = MissingTransactionModel::multi_get_cf(self.db.clone(), tx, operation, BlockHeightCf::name(), &key_prefix, Ordering::Ascending)?;
            for value in values {
                if value.block_height.0 < current_height.as_u64() {
                    let key= MissingTransactionModel::key(&value);
                    MissingTransactionModel::delete(self.db.clone(), tx, operation, &key)?;
                } else {
                    break;
                }
            }
            let block = self.parked_blocks_remove(&block_id.to_string())?;
            return Ok(Some(block));
        }

        Ok(None)
    }

    fn foreign_parked_blocks_insert(&mut self, park_block: &ForeignParkedProposal) -> Result<(), StorageError> {
        let operation = "foreign_parked_blocks_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        ForeignParkedBlockModel::put(self.db.clone(), tx, operation, park_block)?; 

        Ok(())
    }

    fn foreign_parked_blocks_insert_missing_transactions<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        park_block_id: &BlockId,
        missing_transaction_ids: I,
    ) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::{foreign_missing_transactions, foreign_parked_blocks};

        let parked_block_id = foreign_parked_blocks::table
            .select(foreign_parked_blocks::id)
            .filter(foreign_parked_blocks::block_id.eq(serialize_hex(park_block_id)))
            .first::<i32>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_parked_blocks_insert_missing_transactions",
                source: e,
            })?;

        let values = missing_transaction_ids
            .into_iter()
            .map(|tx_id| {
                (
                    foreign_missing_transactions::parked_block_id.eq(parked_block_id),
                    foreign_missing_transactions::transaction_id.eq(serialize_hex(tx_id)),
                )
            })
            .collect::<Vec<_>>();

        diesel::insert_into(foreign_missing_transactions::table)
            .values(values)
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_parked_blocks_insert_missing_transactions",
                source: e,
            })?;

        Ok(())
        */
    }

    fn foreign_parked_blocks_remove_all_by_transaction(
        &mut self,
        transaction_id: &TransactionId,
    ) -> Result<Vec<ForeignParkedProposal>, StorageError> {
        todo!()
        /*
        use crate::schema::{foreign_missing_transactions, foreign_parked_blocks};

        let transaction_id = serialize_hex(transaction_id);

        let removed_ids = diesel::delete(foreign_missing_transactions::table)
            .filter(foreign_missing_transactions::transaction_id.eq(&transaction_id))
            .returning(foreign_missing_transactions::parked_block_id)
            .get_results::<i32>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_parked_blocks_remove_all_by_transaction",
                source: e,
            })?;

        if removed_ids.is_empty() {
            return Ok(vec![]);
        }
        let counts = foreign_parked_blocks::table
            .select((
                foreign_parked_blocks::id,
                foreign_missing_transactions::table
                    .select(count_star())
                    .filter(foreign_missing_transactions::parked_block_id.eq(foreign_parked_blocks::id))
                    .single_value(),
            ))
            .filter(foreign_parked_blocks::id.eq_any(&removed_ids))
            .get_results::<(i32, Option<i64>)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_parked_blocks_remove_all_by_transaction",
                source: e,
            })?;

        let mut remaining = counts
            .iter()
            .filter(|(_, count)| count.map_or(true, |c| c == 0))
            .map(|(id, _)| *id)
            .peekable();

        // If there are still missing transactions for ALL parked blocks, then we exit early
        if remaining.peek().is_none() {
            return Ok(vec![]);
        }

        let blocks = diesel::delete(foreign_parked_blocks::table)
            .filter(foreign_parked_blocks::id.eq_any(remaining))
            .get_results::<sql_models::ForeignParkedBlock>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_parked_blocks_remove_all_by_transaction",
                source: e,
            })?;

        blocks.into_iter().map(TryInto::try_into).collect()
        */
    }

    fn votes_insert(&mut self, vote: &Vote) -> Result<(), StorageError> {
        let operation = "votes_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        VoteModel::put(self.db.clone(), tx, operation, vote)?;
        Ok(())
    }

    fn votes_delete_all(&mut self) -> Result<(), StorageError> {
        let operation = "votes_delete_all";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        VoteModel::delete_all(tx, operation)?;
        Ok(())
    }

    fn substate_locks_insert_all<'a, I: IntoIterator<Item = (&'a SubstateId, &'a Vec<SubstateLock>)>>(
        &mut self,
        block_id: &BlockId,
        locks: I,
    ) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::substate_locks;

        let mut iter = locks.into_iter();
        const CHUNK_SIZE: usize = 100;
        // We have to break up into multiple queries because we can hit max SQL variable limit
        loop {
            let locks = iter
                .by_ref()
                .take(CHUNK_SIZE)
                .flat_map(|(id, locks)| {
                    let block_id = serialize_hex(block_id);
                    locks.iter().map(move |lock| {
                        (
                            substate_locks::block_id.eq(block_id.clone()),
                            substate_locks::substate_id.eq(id.to_string()),
                            substate_locks::version.eq(lock.version() as i32),
                            substate_locks::transaction_id.eq(serialize_hex(lock.transaction_id())),
                            substate_locks::lock.eq(lock.substate_lock().to_string()),
                            substate_locks::is_local_only.eq(lock.is_local_only()),
                        )
                    })
                })
                .collect::<Vec<_>>();

            let count = locks.len();
            if count == 0 {
                break;
            }

            diesel::insert_into(substate_locks::table)
                .values(locks)
                .execute(self.connection())
                .map_err(|e| SqliteStorageError::DieselError {
                    operation: "substate_locks_insert_all",
                    source: e,
                })?;

            if count < CHUNK_SIZE {
                break;
            }
        }

        Ok(())
        */
    }

    fn substate_locks_remove_many_for_transactions<'a, I: Iterator<Item = &'a TransactionId>>(
        &mut self,
        mut transaction_ids: Peekable<I>,
    ) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::substate_locks;

        // NOTE: looked at the diesel code and if the iterator is empty, this executes WHERE 0=1 which is fine, but
        // let's check the peekable iterator to save an OP.
        if transaction_ids.peek().is_none() {
            return Ok(());
        }

        diesel::delete(substate_locks::table)
            .filter(substate_locks::transaction_id.eq_any(transaction_ids.map(serialize_hex)))
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "substate_locks_release_all_by_substates",
                source: e,
            })?;

        Ok(())
        */
    }

    fn substate_locks_remove_any_by_block_id(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::substate_locks;

        diesel::delete(substate_locks::table)
            .filter(substate_locks::block_id.eq(serialize_hex(block_id)))
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "substate_locks_remove_any_by_block_id",
                source: e,
            })?;

        Ok(())
        */
    }

    fn substates_create(&mut self, substate: &SubstateRecord) -> Result<(), StorageError> {
        if substate.is_destroyed() {
            return Err(StorageError::QueryError {
                reason: format!(
                    "calling substates_create with a destroyed SubstateRecord is not valid. substate_id = {}",
                    &substate.substate_id
                ),
            });
        }

        let operation = "substates_create";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        SubstateModel::put(self.db.clone(), tx, operation, &substate)?;

        // Calculate the index ("seq" field) of the state transition for the shard
        type ShardCf = crate::model::state_transition::ShardColumnFamily;
        let key_prefix = ShardCf::build_key_prefix_by_shard(&substate.created_by_shard);
        // TODO: this could be optimized by a new model function that allows to specify the we only want one key
        let shard_transitions = StateTransitionModel::multi_get_cf(self.db.clone(), tx, operation, ShardCf::name(), &key_prefix, Ordering::Descending)?;
        let next_seq = match shard_transitions.first() {
            Some(value) => {
                value.seq.0
            },
            None => 1,
        };

        // Insert the next state transition
        let state_transition = StateTransitionModelData::new(
            StateTransition {
                id: StateTransitionId::new(substate.created_at_epoch, substate.created_by_shard, next_seq),
                update: SubstateUpdate::Create(SubstateCreatedProof {
                    substate: SubstateData {
                        substate_id: substate.substate_id.clone(),
                        version: substate.version,
                        substate_value: substate.substate_value.clone(),
                        created_by_transaction: substate.created_by_transaction,
                    },
                }),
            },
            substate.created_by_shard,
            next_seq,
        )?;
        StateTransitionModel::put(self.db.clone(), tx, operation, &state_transition)?;

        Ok(())
    }

    fn substates_down(
        &mut self,
        versioned_substate_id: VersionedSubstateId,
        shard: Shard,
        epoch: Epoch,
        destroyed_block_height: NodeHeight,
        destroyed_transaction_id: &TransactionId,
        destroyed_qc_id: &QcId,
    ) -> Result<(), StorageError> {
        let operation = "substates_down";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        
        // update the substate
        let address = versioned_substate_id.to_substate_address();
        let key = SubstateModel::key_from_address(&address);
        let mut substate = SubstateModel::get(tx, operation, &key)?;
        substate.destroyed = Some(SubstateDestroyed {
            by_transaction: *destroyed_transaction_id,
            justify: *destroyed_qc_id,
            by_block: destroyed_block_height,
            at_epoch: epoch,
            by_shard: shard,
        });
        SubstateModel::put(self.db.clone(), tx, operation, &substate)?;

        // Calculate the index ("seq" field) of the state transition
        type ShardCf = crate::model::state_transition::ShardColumnFamily;
        let key_prefix = ShardCf::build_key_prefix_by_shard(&substate.created_by_shard);
        // TODO: this could be optimized by a new model function that allows to specify the we only want one key
        let shard_transitions = StateTransitionModel::multi_get_cf(self.db.clone(), tx, operation, ShardCf::name(), &key_prefix, Ordering::Descending)?;
        let next_seq = match shard_transitions.first() {
            Some(value) => {
                value.seq.0
            },
            None => 1,
        };

        // insert new state transition down
        let state_transition = StateTransitionModelData::new(
            StateTransition {
                id: StateTransitionId::new(epoch, shard, next_seq),
                update: SubstateUpdate::Destroy(
                    SubstateDestroyedProof {
                        substate_id: versioned_substate_id.substate_id,
                        version: versioned_substate_id.version,
                        destroyed_by_transaction: *destroyed_transaction_id,
                    }
                ),
            },
            shard,
            next_seq,
        )?;
        StateTransitionModel::put(self.db.clone(), tx, operation, &state_transition)?;

        Ok(())
    }

    fn foreign_substate_pledges_save(
        &mut self,
        transaction_id: &TransactionId,
        // TODO: seems like this field is not really needed/used?
        _shard_group: ShardGroup,
        pledges: &SubstatePledges,
    ) -> Result<(), StorageError> {
        let operation = "foreign_substate_pledges_save";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        
        for pledge in pledges {
            let value = ForeignSubstatePledgeData {
                transaction_id: *transaction_id,
                substate_address: pledge.to_substate_address(),
                pledge: pledge.clone(),
            };
            ForeignSubstatePledgeModel::put(self.db.clone(), tx, operation, &value)?;
        }

        Ok(())
    }

    fn foreign_substate_pledges_remove_many<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        let operation = "foreign_substate_pledges_remove_many";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        
        for transaction_id in transaction_ids {
            let key_prefix = ForeignSubstatePledgeModel::key_from_transaction_and_address(transaction_id, None);
            let pledges = ForeignSubstatePledgeModel::multi_get(tx, Some(&key_prefix), Ordering::Ascending)?;
            for pledge in pledges {
                let key = ForeignSubstatePledgeModel::key(&pledge);
                ForeignSubstatePledgeModel::delete(self.db.clone(), tx, operation, &key)?;
            }
        }

        Ok(())
    }

    fn pending_state_tree_diffs_insert(
        &mut self,
        block_id: BlockId,
        shard: Shard,
        diff: &VersionedStateHashTreeDiff,
    ) -> Result<(), StorageError> {
        let operation = "pending_state_tree_diffs_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // Get the corresponding block height
        let block_key = BlockModel::key_from_block_id(&block_id);
        let block = BlockModel::get(tx, operation, &block_key)?;
        let block_height = block.height();

        let value = PendingStateTreeDiffData {
            block_id,
            block_height,
            shard,
            diff: diff.clone(),
        };
        PendingStateTreeDiffModel::put(self.db.clone(), tx, operation, &value)?;

        Ok(())
    }

    fn pending_state_tree_diffs_remove_by_block(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        let operation = "pending_state_tree_diffs_remove_by_block";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key_prefix = PendingStateTreeDiffModel::key_from_block_and_height(block_id, None);
        let values = PendingStateTreeDiffModel::multi_get(tx, Some(&key_prefix), Ordering::Ascending)?;
        for value in values {
            let key = PendingStateTreeDiffModel::key(&value);
            PendingStateTreeDiffModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        Ok(())
    }

    fn pending_state_tree_diffs_remove_and_return_by_block(
        &mut self,
        block_id: &BlockId,
    ) -> Result<IndexMap<Shard, Vec<PendingShardStateTreeDiff>>, StorageError> {
        let operation = "pending_state_tree_diffs_remove_and_return_by_block";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        // get all diff records from database
        let key_prefix = PendingStateTreeDiffModel::key_from_block_and_height(block_id, None);
        let diff_recs = PendingStateTreeDiffModel::multi_get(tx, Some(&key_prefix), Ordering::Ascending)?;

        // delete all of them from database
        for diff in &diff_recs {
            let key = PendingStateTreeDiffModel::key(&diff);
            PendingStateTreeDiffModel::delete(self.db.clone(), tx, operation, &key)?;
        }

        // create and return an indexmap with all the diff recors
        let mut diffs = IndexMap::new();
        for diff in diff_recs {
            let key = diff.shard;
            let value = PendingShardStateTreeDiff::from(diff);
            diffs.entry(key).or_insert_with(Vec::new).push(value);
        }

        Ok(diffs)
    }

    fn state_tree_nodes_insert(&mut self, shard: Shard, key: NodeKey, node: Node<Version>) -> Result<(), StorageError> {
        let operation = "state_tree_nodes_insert";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        
        let value = StateTreeModelData {
            shard,
            key,
            node,
        };

        StateTreeModel::put(self.db.clone(), tx, operation, &value)?;

        Ok(())
    }

    fn state_tree_nodes_record_stale_tree_node(
        &mut self,
        shard: Shard,
        node: StaleTreeNode,
    ) -> Result<(), StorageError> {
        let operation = "state_tree_nodes_record_stale_tree_node";
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();

        let key = StateTreeModel::key_from_shard_and_node(&shard, node.as_node_key());
        StateTreeModel::delete(self.db.clone(), tx, operation, &key)?;

        Ok(())
    }

    fn state_tree_shard_versions_set(&mut self, shard: Shard, version: Version) -> Result<(), StorageError> {    
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        let value = StateTreeShardVersionModelData {
            shard,
            version
        };
        Ok(StateTreeShardVersionModel::put(self.db.clone(), tx, "state_tree_shard_versions_set", &value)?)
    }

    fn epoch_checkpoint_save(&mut self, checkpoint: &EpochCheckpoint) -> Result<(), StorageError> {
        let tx = self.transaction.as_mut().unwrap().rocksdb_transaction();
        Ok(EpochCheckpointModel::put(self.db.clone(), tx, "epoch_checkpoint_save", checkpoint)?)
    }

    fn burnt_utxos_insert(&mut self, burnt_utxo: &BurntUtxo) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::burnt_utxos;

        let values = (
            burnt_utxos::substate_id.eq(burnt_utxo.substate_id.to_string()),
            burnt_utxos::substate.eq(serialize_json(&burnt_utxo.substate_value)?),
            burnt_utxos::base_layer_block_height.eq(burnt_utxo.base_layer_block_height as i64),
        );

        diesel::insert_into(burnt_utxos::table)
            .values(values)
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "burnt_utxos_insert",
                source: e,
            })?;

        Ok(())
        */
    }

    fn burnt_utxos_set_proposed_block(
        &mut self,
        commitment: &UnclaimedConfidentialOutputAddress,
        proposed_in_block: &BlockId,
    ) -> Result<(), StorageError> {
        todo!()
    }

    fn burnt_utxos_clear_proposed_block(&mut self, proposed_in_block: &BlockId) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::burnt_utxos;

        let proposed_in_block_hex = serialize_hex(proposed_in_block);
        diesel::update(burnt_utxos::table)
            .filter(burnt_utxos::proposed_in_block.eq(&proposed_in_block_hex))
            .set((
                burnt_utxos::proposed_in_block.eq(None::<String>),
                burnt_utxos::proposed_in_block_height.eq(None::<i64>),
            ))
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "burnt_utxos_clear_proposed_block",
                source: e,
            })?;

        Ok(())
        */
    }

    fn burnt_utxos_delete(&mut self, commitment: &UnclaimedConfidentialOutputAddress) -> Result<(), StorageError> {
        todo!()
    }

    fn lock_conflicts_insert_all<'a, I: IntoIterator<Item = (&'a TransactionId, &'a Vec<LockConflict>)>>(
        &mut self,
        block_id: &BlockId,
        conflicts: I,
    ) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::lock_conflicts;

        let values = conflicts
            .into_iter()
            .flat_map(|(tx_id, conflicts)| {
                conflicts.iter().map(move |conflict| {
                    (
                        lock_conflicts::block_id.eq(serialize_hex(block_id)),
                        lock_conflicts::transaction_id.eq(serialize_hex(tx_id)),
                        lock_conflicts::depends_on_tx.eq(serialize_hex(conflict.transaction_id)),
                        lock_conflicts::lock_type.eq(conflict.requested_lock.to_string()),
                    )
                })
            })
            .collect::<Vec<_>>();

        diesel::insert_into(lock_conflicts::table)
            .values(values)
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "lock_conflicts_insert_all",
                source: e,
            })?;

        Ok(())
        */
    }

    fn validator_epoch_stats_add_participation_share(&mut self, qc_id: &QcId) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::{quorum_certificates, validator_epoch_stats};

        let qc_id = serialize_hex(qc_id);
        let qc_json = quorum_certificates::table
            .select(quorum_certificates::json)
            .filter(quorum_certificates::qc_id.eq(&qc_id))
            .filter(quorum_certificates::is_shares_processed.eq(false))
            .first::<String>(self.connection())
            .optional()
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "validator_epoch_stats_add_participation_share",
                source: e,
            })?;
        let Some(qc_json) = qc_json else {
            return Ok(());
        };

        let qc = deserialize_json::<QuorumCertificate>(&qc_json)?;
        let epoch = qc.epoch().as_u64() as i64;

        for sig in qc.signatures() {
            let values = (
                validator_epoch_stats::epoch.eq(epoch),
                validator_epoch_stats::public_key.eq(serialize_hex(sig.public_key().as_bytes())),
                validator_epoch_stats::participation_shares.eq(1),
            );

            diesel::insert_into(validator_epoch_stats::table)
                .values(values)
                .on_conflict((validator_epoch_stats::epoch, validator_epoch_stats::public_key))
                .do_update()
                .set(validator_epoch_stats::participation_shares.eq(validator_epoch_stats::participation_shares + 1))
                .execute(self.connection())
                .map_err(|e| SqliteStorageError::DieselError {
                    operation: "validator_epoch_stats_add_participation_share",
                    source: e,
                })?;
        }

        // Mark QC shares as processed
        diesel::update(quorum_certificates::table)
            .filter(quorum_certificates::qc_id.eq(qc_id))
            .set(quorum_certificates::is_shares_processed.eq(true))
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "validator_epoch_stats_add_participation_share",
                source: e,
            })?;

        Ok(())
        */
    }

    fn validator_epoch_stats_updates<'a, I: IntoIterator<Item = ValidatorStatsUpdate<'a>>>(
        &mut self,
        epoch: Epoch,
        updates: I,
    ) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::validator_epoch_stats;

        let epoch = epoch.as_u64() as i64;

        for update in updates {
            let existing = validator_epoch_stats::table
                .select((
                    validator_epoch_stats::participation_shares,
                    validator_epoch_stats::missed_proposals,
                ))
                .filter(validator_epoch_stats::epoch.eq(epoch))
                .filter(validator_epoch_stats::public_key.eq(serialize_hex(update.public_key().as_bytes())))
                .first::<(i64, i64)>(self.connection())
                .optional()
                .map_err(|e| SqliteStorageError::DieselError {
                    operation: "validator_epoch_stats_updates",
                    source: e,
                })?;

            match existing {
                Some((participation_shares, missed_proposals)) => match update.missed_proposal_change() {
                    Some(0) => {
                        diesel::update(validator_epoch_stats::table)
                            .filter(validator_epoch_stats::epoch.eq(epoch))
                            .filter(validator_epoch_stats::public_key.eq(serialize_hex(update.public_key().as_bytes())))
                            .set((
                                validator_epoch_stats::participation_shares
                                    .eq(participation_shares + update.participation_shares_increment() as i64),
                                validator_epoch_stats::missed_proposals.eq(0),
                            ))
                            .execute(self.connection())
                            .map_err(|e| SqliteStorageError::DieselError {
                                operation: "validator_epoch_stats_updates",
                                source: e,
                            })?;
                    },
                    Some(n) => {
                        let missed_proposal_count = update
                            .max_total_missed_proposals()
                            .min(cmp::max(missed_proposals + n, 0));
                        diesel::update(validator_epoch_stats::table)
                            .filter(validator_epoch_stats::epoch.eq(epoch))
                            .filter(validator_epoch_stats::public_key.eq(serialize_hex(update.public_key().as_bytes())))
                            .set((
                                validator_epoch_stats::participation_shares
                                    .eq(participation_shares + update.participation_shares_increment() as i64),
                                validator_epoch_stats::missed_proposals.eq(missed_proposal_count),
                            ))
                            .execute(self.connection())
                            .map_err(|e| SqliteStorageError::DieselError {
                                operation: "validator_epoch_stats_updates",
                                source: e,
                            })?;
                    },

                    None => {
                        diesel::update(validator_epoch_stats::table)
                            .filter(validator_epoch_stats::epoch.eq(epoch))
                            .filter(validator_epoch_stats::public_key.eq(serialize_hex(update.public_key().as_bytes())))
                            .set(
                                validator_epoch_stats::participation_shares
                                    .eq(participation_shares + update.participation_shares_increment() as i64),
                            )
                            .execute(self.connection())
                            .map_err(|e| SqliteStorageError::DieselError {
                                operation: "validator_epoch_stats_updates",
                                source: e,
                            })?;
                    },
                },
                None => {
                    let leader_failure_inc = update.missed_proposal_change().map_or(0i64, |set| set.max(0));
                    let values = (
                        validator_epoch_stats::epoch.eq(epoch),
                        validator_epoch_stats::public_key.eq(serialize_hex(update.public_key().as_bytes())),
                        validator_epoch_stats::participation_shares.eq(update.participation_shares_increment() as i64),
                        validator_epoch_stats::missed_proposals.eq(leader_failure_inc),
                    );

                    diesel::insert_into(validator_epoch_stats::table)
                        .values(values)
                        .execute(self.connection())
                        .map_err(|e| SqliteStorageError::DieselError {
                            operation: "validator_epoch_stats_updates",
                            source: e,
                        })?;
                },
            }
        }

        Ok(())
        */
    }

    fn diagnostics_add_no_vote(&mut self, block_id: BlockId, reason: NoVoteReason) -> Result<(), StorageError> {
        todo!()
        /*
        use crate::schema::{blocks, diagnostics_no_votes};
        let block_id = serialize_hex(block_id);

        let values = (
            diagnostics_no_votes::block_id.eq(&block_id),
            diagnostics_no_votes::block_height.eq(blocks::table
                .select(blocks::height)
                .filter(blocks::block_id.eq(&block_id))
                .single_value()
                .assume_not_null()),
            diagnostics_no_votes::reason_code.eq(reason.as_code_str()),
            diagnostics_no_votes::reason_text.eq(reason.to_string()),
        );

        diesel::insert_into(diagnostics_no_votes::table)
            .values(values)
            .execute(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "diagnostics_add_no_vote",
                source: e,
            })?;

        Ok(())
        */
    }
    
    fn lock_conflicts_remove_by_transaction_ids<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        todo!()
    }
    
    fn lock_conflicts_remove_by_block_id(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        todo!()
    }
    
    fn evicted_nodes_evict(&mut self, public_key: &PublicKey, evicted_in_block: BlockId) -> Result<(), StorageError> {
        todo!()
    }
    
    fn evicted_nodes_mark_eviction_as_committed(
        &mut self,
        public_key: &PublicKey,
        epoch: Epoch,
    ) -> Result<(), StorageError> {
        todo!()
    }
}

impl<'a, TAddr> Deref for RocksDbStateStoreWriteTransaction<'a, TAddr> {
    type Target = RocksDbStateStoreReadTransaction<'a, TAddr>;

    fn deref(&self) -> &Self::Target {
        self.transaction.as_ref().unwrap()
    }
}

impl<TAddr> Drop for RocksDbStateStoreWriteTransaction<'_, TAddr> {
    fn drop(&mut self) {
        if self.transaction.is_some() {
            warn!(
                target: LOG_TARGET,
                "Shard store write transaction was not committed/rolled back"
            );
        }
    }
}

fn now() -> PrimitiveDateTime {
    let now = time::OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}
