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

use std::{
    borrow::Borrow, cell::UnsafeCell, collections::{HashMap, HashSet}, marker::PhantomData, ops::RangeInclusive, sync::{Arc, Mutex, MutexGuard}, thread::current
};

use bigdecimal::{BigDecimal, ToPrimitive};
use indexmap::IndexMap;
use log::*;
use rocksdb::{Transaction, TransactionDB};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::value::Index;
use tari_common_types::types::{FixedHash, PublicKey};
use tari_dan_common_types::{
    shard::Shard,
    Epoch,
    NodeAddressable,
    NodeHeight,
    ShardGroup,
    SubstateAddress,
    SubstateRequirement,
    ToSubstateAddress,
    VersionedSubstateId, VersionedSubstateIdRef,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockDiff,
        BlockId,
        BlockTransactionExecution,
        BurntUtxo,
        Command,
        EpochCheckpoint,
        ForeignProposal,
        ForeignProposalAtom,
        ForeignProposalStatus,
        ForeignReceiveCounters,
        ForeignSendCounters,
        HighQc,
        LastExecuted,
        LastProposed,
        LastSentVote,
        LastVoted,
        LeafBlock,
        LockedBlock,
        LockedSubstateValue,
        PendingShardStateTreeDiff,
        QcId,
        QuorumCertificate,
        StateTransition,
        StateTransitionId,
        SubstateChange,
        SubstateLock,
        SubstatePledge,
        SubstatePledges,
        SubstateRecord,
        TransactionPoolConfirmedStage,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
        VersionedSubstateIdLockIntent,
        Vote,
    },
    Ordering,
    StateStoreReadTransaction,
    StorageError,
};
use tari_engine_types::{substate::SubstateId, template_models::UnclaimedConfidentialOutputAddress};
use tari_state_tree::{Node, NodeKey, TreeNode, Version};
use tari_transaction::TransactionId;
use tari_utilities::{hex::Hex, ByteArray};
use tari_dan_storage::consensus_models::ValidatorConsensusStats;

use crate::{error::RocksDbStorageError, model::{self, block::BlockModel, block_diff::{BlockDiffData, BlockDiffModel}, block_transaction_execution::{BlockTransactionExecutionModel, BlockTransactionExecutionModelData}, burnt_utxo::BurntUtxoModel, epoch_checkpoint::EpochCheckpointModel, evicted_node::EvictedNodeModel, foreign_parked_blocks::ForeignParkedBlockModel, foreign_proposal::ForeignProposalModel, foreign_receive_counter::ForeignReceiveCounterModel, foreign_send_counter::ForeignSendCounterModel, foreign_substate_pledge::ForeignSubstatePledgeModel, high_qc::HighQcModel, last_executed::LastExecutedModel, last_proposed::LastProposedModel, last_sent_vote::LastSentVoteModel, last_voted::LastVotedModel, leaf_block::LeafBlockModel, locked_block::LockedBlockModel, missing_transactions::MissingTransactionModel, model::{ModelColumnFamily, RocksdbModel}, pending_state_tree_diff::PendingStateTreeDiffModel, quorum_certificate::QuorumCertificateModel, state_tree::StateTreeModel, state_tree_shard_versions::StateTreeShardVersionModel, substate::SubstateModel, substate_locks::SubstateLockModel, transaction::TransactionModel, transaction_pool::TransactionPoolModel, transaction_pool_state_update::{TransactionPoolStateUpdateModel, TransactionPoolStateUpdateModelData}, vote::VoteModel}};

const LOG_TARGET: &str = "tari::dan::storage::state_store_rocksdb::reader";

pub struct RocksDbStateStoreReadTransaction<'a, TAddr> {
    //transaction: RocksDbTransaction<'a>,
    //db: MutexGuard<'a, TransactionDB>,
    //tx: UnsafeCell<MutexGuard<'a, Transaction<'a, TransactionDB>>>,
    tx:  Transaction<'a, TransactionDB>,
    db: Arc<TransactionDB>,
    _addr: PhantomData<TAddr>,
}

impl<'a, TAddr> RocksDbStateStoreReadTransaction<'a, TAddr> {
    pub(crate) fn new(db: Arc<TransactionDB>, tx: Transaction<'a, TransactionDB>) -> Self {
        Self {
            tx,
            db,
            _addr: PhantomData,
        }
    }

    pub(crate) fn rocksdb_transaction(&mut self) -> &mut Transaction<'a, TransactionDB> {
        &mut self.tx
    }

    pub(crate) fn commit(self) -> Result<(), RocksDbStorageError> {
        self.tx.commit()
        .map_err(|source| RocksDbStorageError::RocksDbError {
            source,
            operation: "commit",
        })?;
        Ok(())
    }

    pub(crate) fn rollback(self) -> Result<(), RocksDbStorageError> {
        self.tx.rollback()
        .map_err(|source| RocksDbStorageError::RocksDbError {
            source,
            operation: "commit",
        })?;
        Ok(())
    }
}

impl<'a, TAddr: NodeAddressable + Serialize + DeserializeOwned + 'a> RocksDbStateStoreReadTransaction<'a, TAddr> {
    pub(crate) fn get_transaction_atom_state_updates_between_blocks<'i, ITx>(
        &self,
        from_block_id: &BlockId,
        to_block_id: &BlockId,
        transaction_ids: ITx,
    ) -> Result<IndexMap<String, TransactionPoolStateUpdateModelData>, RocksDbStorageError>
    where
        ITx: Iterator<Item = &'i str> + ExactSizeIterator,
    {
        if transaction_ids.len() == 0 {
            return Ok(IndexMap::new());
        }

        // Blocks without commands may change pending transaction state because they justify a
        // block that proposes a change. So we cannot only use blocks that have commands.
        let applicable_block_ids = self.get_block_ids_between(from_block_id, to_block_id)?;

        debug!(
            target: LOG_TARGET,
            "get_transaction_atom_state_updates_between_blocks: from_block_id={}, to_block_id={}, len(applicable_block_ids)={}",
            from_block_id,
            to_block_id,
            applicable_block_ids.len());

        if applicable_block_ids.is_empty() {
            return Ok(IndexMap::new());
        }

        let block_ids = applicable_block_ids.iter().map(|s| s.as_str());
        self.get_ranked_transaction_atom_updates(transaction_ids, block_ids)
    }

    /// Creates a query to select the latest transaction pool state updates for the given transaction ids and block ids.
    /// If no transaction ids are provided, all updates for the given block ids are returned.
    fn get_ranked_transaction_atom_updates<
        'i1,
        'i2,
        IBlk: Iterator<Item = &'i1 str> + ExactSizeIterator,
        ITx: Iterator<Item = &'i2 str> + ExactSizeIterator,
    >(
        &self,
        transaction_ids: ITx,
        block_ids: IBlk,
    ) -> Result<IndexMap<String, TransactionPoolStateUpdateModelData>, RocksDbStorageError>
    {
        // TODO: optimize this query in RocksDB
        let transaction_ids: Vec<String> = transaction_ids.map(|id| id.to_string()).collect();
        let mut res = IndexMap::new();
        for block_id in block_ids {
            let key_value = TransactionPoolStateUpdateModel::key_prefix_by_block_id_str(block_id);
            let updates = TransactionPoolStateUpdateModel::multi_get( &self.tx, Some(&key_value), Ordering::Ascending)?;
            updates
                .iter()
                .filter(|u| {
                    if !transaction_ids.is_empty() && !transaction_ids.contains(&u.transaction_id.to_string()) {
                        return false;
                    }
                    u.is_applied == false
                })
                .for_each(|u| {
                    res.insert(u.transaction_id.to_string(), u.clone());
                });
        }

        Ok(res)
    }

    /// Returns the blocks from the start_block (inclusive) to the end_block (inclusive).
    fn get_block_ids_between(
        &self,
        start_block: &BlockId,
        end_block: &BlockId,
    ) -> Result<Vec<String>, RocksDbStorageError> {
        debug!(target: LOG_TARGET, "get_block_ids_between: start: {start_block}, end: {end_block}");
        
        let mut block_ids = vec![];

        let mut block_id = *end_block;
        while block_id != *start_block && block_id != BlockId::genesis() {
            let key: String = BlockModel::key_from_block_id(&block_id);
            let block = BlockModel::get(&self.tx, "get_block_ids_between", &key)?;
            block_ids.push(block.id().to_string());
            block_id = *block.parent();
        }

        Ok(block_ids)
    }

    pub(crate) fn get_block_ids_with_commands_between(
        &self,
        start_block: &BlockId,
        end_block: &BlockId,
    ) -> Result<Vec<String>, RocksDbStorageError> {
        todo!()
        /*
        let block_ids = sql_query(
            r#"
            WITH RECURSIVE tree(bid, parent, is_dummy, command_count) AS (
                SELECT block_id, parent_block_id, is_dummy, command_count FROM blocks where block_id = ?
            UNION ALL
                SELECT block_id, parent_block_id, blocks.is_dummy, blocks.command_count
                FROM blocks JOIN tree ON
                    block_id = tree.parent
                    AND tree.bid != ?
                    AND tree.parent != '0000000000000000000000000000000000000000000000000000000000000000'
                LIMIT 1000
            )
            SELECT bid FROM tree where is_dummy = 0 AND command_count > 0"#,
        )
        .bind::<Text, _>(serialize_hex(end_block))
        .bind::<Text, _>(serialize_hex(start_block))
        .load_iter::<BlockIdSqlValue, _>(self.connection())
        .map_err(|e| SqliteStorageError::DieselError {
            operation: "get_block_ids_that_change_state_between",
            source: e,
        })?;

        block_ids
            .map(|b| {
                b.map(|b| b.bid).map_err(|e| SqliteStorageError::DieselError {
                    operation: "get_block_ids_that_change_state_between",
                    source: e,
                })
            })
            .collect()
         */
    }

    /// Used in tests, therefore not used in consensus and not part of the trait
    pub fn transactions_count(&self) -> Result<u64, RocksDbStorageError> {
        todo!()
    }

    pub(crate) fn get_commit_block_id(&self) -> Result<BlockId, StorageError> {
        let block_opt = BlockModel::get_cf(self.db.clone(), &self.tx, model::block::IsCommittedColumnFamily::NAME, "get_commit_block_id", None, Ordering::Descending)?;
        match block_opt {
            Some(block) => Ok(*block.id()),
            None => Err(StorageError::General {
                details: "get_commit_block_id: no commited block found".to_string() 
            })
        }
    }

    pub fn substates_count(&self) -> Result<u64, RocksDbStorageError> {
        Ok(SubstateModel::count(&self.tx, None)?)
    }

    pub fn blocks_get_tip(&self, epoch: Epoch, shard_group: ShardGroup) -> Result<Block, StorageError> {
        todo!()
        /*
        use crate::schema::{blocks, quorum_certificates};

        let (block, qc) = blocks::table
            .left_join(quorum_certificates::table.on(blocks::qc_id.eq(quorum_certificates::qc_id)))
            .select((blocks::all_columns, quorum_certificates::all_columns.nullable()))
            .filter(blocks::epoch.eq(epoch.as_u64() as i64))
            .filter(blocks::shard_group.eq(shard_group.encode_as_u32() as i32))
            .order_by(blocks::height.desc())
            .first::<(sql_models::Block, Option<sql_models::QuorumCertificate>)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "blocks_get_tip",
                source: e,
            })?;

        let qc = qc.ok_or_else(|| SqliteStorageError::DbInconsistency {
            operation: "blocks_get_tip",
            details: format!(
                "block {} references non-existent quorum certificate {}",
                block.block_id, block.qc_id
            ),
        })?;

        block.try_convert(qc)
        */
    }

    fn get_current_locked_block(&self) -> Result<LockedBlock, StorageError> {
        let value = LockedBlockModel::get_first(&self.tx, "get_current_locked_block", None, Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No locked block stored in database".to_string() })?;
        Ok(value.locked_block)
    }
}

impl<'tx, TAddr: NodeAddressable + Serialize + DeserializeOwned + 'tx> StateStoreReadTransaction
    for RocksDbStateStoreReadTransaction<'tx, TAddr>
{
    type Addr = TAddr;

    fn last_sent_vote_get(&self) -> Result<LastSentVote, StorageError> {
        let value = LastSentVoteModel::get_first(&self.tx, "last_sent_vote_get", None, Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No last sent vote stored in database".to_string() })?;
        Ok(value)
    }

    fn last_voted_get(&self) -> Result<LastVoted, StorageError> {
        type Cf = crate::model::last_voted::TimestampColumnFamily;
        let cf = Cf::name();

        let value = LastVotedModel::
            get_cf(self.db.clone(), &self.tx, cf, "last_voted_get", None, Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No last voted stored in database".to_string() })?;

        Ok(value.last_voted)
    }

    fn last_executed_get(&self) -> Result<LastExecuted, StorageError> {
        let value = LastExecutedModel::get_first(&self.tx, "last_executed_get", None, Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No last executed stored in database".to_string() })?;
        Ok(value)
    }

    fn last_proposed_get(&self) -> Result<LastProposed, StorageError> {
        type Cf = crate::model::last_proposed::TimestampColumnFamily;
        let cf = Cf::name();

        let value = LastProposedModel::
            get_cf(self.db.clone(), &self.tx, cf, "last_proposed_get", None, Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No last executed stored in database".to_string() })?;

        Ok(value.last_proposed)
    }

    fn locked_block_get(&self, epoch: Epoch) -> Result<LockedBlock, StorageError> {
        let key_prefix = LockedBlockModel::key_prefix_by_epoch(epoch);
        let value = LockedBlockModel::get_first(&self.tx, "locked_block_get", Some(&key_prefix), Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No locked block stored in database".to_string() })?;
        Ok(value.locked_block)
    }

    fn leaf_block_get(&self, epoch: Epoch) -> Result<LeafBlock, StorageError> {
        let key_prefix = LeafBlockModel::key_prefix_by_epoch(epoch);
        let value = LeafBlockModel::get_first(&self.tx, "leaf_block_get", Some(&key_prefix), Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No leaf block stored in database".to_string() })?;
        Ok(value.leaf_block)
    }

    fn high_qc_get(&self, epoch: Epoch) -> Result<HighQc, StorageError> {
        let key_prefix = HighQcModel::key_prefix_by_epoch(epoch);
        let value = HighQcModel::get_first(&self.tx, "high_qc_get", Some(&key_prefix), Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No high qc stored in database".to_string() })?;
        Ok(value.high_qc)
    }

    fn foreign_proposals_get_any<'a, I: IntoIterator<Item = &'a BlockId>>(
        &self,
        block_ids: I,
    ) -> Result<Vec<ForeignProposal>, StorageError> {
        let mut proposals = vec![];

        for block_id in block_ids {
            let key = ForeignProposalModel::key_from_block_id(block_id);
            let res = ForeignProposalModel::get_first(&self.tx, "foreign_proposals_get_any", Some(&key), Ordering::Descending)?;
            if let Some(proposal) = res {
                proposals.push(proposal);
            }
        }

        // TODO: should we fetch the related quorum certificate like the sqlite implementation does?

        Ok(proposals)
    }

    fn foreign_proposals_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        let key = ForeignProposalModel::key_from_block_id(block_id);
        let proposal_exists = ForeignProposalModel::key_exists(&self.tx, "foreign_proposals_exists", &key)?;
        Ok(proposal_exists)
    }

    fn foreign_proposals_has_unconfirmed(&self, epoch: Epoch) -> Result<bool, StorageError> {
        type Cf = crate::model::foreign_proposal::UnconfirmedColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::key_prefix_by_epoch(&epoch);
        let res = ForeignProposalModel::get_cf(self.db.clone(), &self.tx, cf, "foreign_proposals_has_unconfirmed", Some(&key_prefix), Ordering::Ascending)?;
        
        Ok(res.is_some())
    }

    fn foreign_proposals_get_all_new(
        &self,
        block_id: &BlockId,
        limit: usize,
    ) -> Result<Vec<ForeignProposal>, StorageError> {
        let operation = "foreign_proposals_get_all_new";

        if !self.blocks_exists(block_id)? {
            return Err(StorageError::NotFound {
                item: "foreign_proposals_get_all_new: Block",
                key: block_id.to_string(),
            });
        }

        let locked = self.get_current_locked_block()?;
        let pending_block_ids = self.get_block_ids_with_commands_between(&locked.block_id, block_id)?;

        type Cf = crate::model::foreign_proposal::EpochStatusColumnFamily;
        let cf = Cf::name();

        let mut proposals = HashMap::new();

        // get all proposals with status "New"
        let key_prefix = Cf::key_prefix_from_epoch_and_status(&locked.epoch, &ForeignProposalStatus::New);
        let new_proposals = ForeignProposalModel::multi_get_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix, Ordering::Ascending)?;
        proposals.extend(new_proposals
            .into_iter()
            .map(|f| (f.block.id().clone(), f)));

        // get all proposals with status "Proposed"
        let key_prefix = Cf::key_prefix_from_epoch_and_status(&locked.epoch, &ForeignProposalStatus::Proposed);
        let proposed_proposals = ForeignProposalModel::multi_get_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix, Ordering::Ascending)?;
        proposals.extend(proposed_proposals
            .into_iter()
            .map(|f| (f.block.id().clone(), f)));

        let proposals: Vec<ForeignProposal> = proposals
            .into_values()
            .into_iter()
            // we don't want proposals that are proposed in pending blocks 
            .filter(|p| {
                let Some(proposed_by) = p.proposed_by_block  else {
                    return true;
                };
                !pending_block_ids.contains(&proposed_by.to_string())
            })
            // TODO: do we need to filter by proposal.proposed_in_block_height > locked.height?. We will need to store proposed_in_block_height field
            .collect();

        // TODO: use "limit" in rocksdb

        Ok(proposals)    
    }

    fn foreign_proposal_get_all_pending(
        &self,
        from_block_id: &BlockId,
        to_block_id: &BlockId,
    ) -> Result<Vec<ForeignProposalAtom>, StorageError> {
        let operation = "foreign_proposal_get_all_pending";

        let mut all_commands = vec![];

        let block_ids = self.get_block_ids_with_commands_between(from_block_id, to_block_id)?;
        for block_id in block_ids {
            let key = BlockModel::key_from_block_id_str(&block_id);
            let block = BlockModel::get(&self.tx, operation, &key)?;

            if block.command_count() > 0 {
                for command in block.commands() {
                    all_commands.push(command.clone());
                }
            }
        }

        all_commands.dedup();

        Ok(all_commands
            .into_iter()
            .filter_map(|command| command.foreign_proposal().cloned())
            .collect::<Vec<ForeignProposalAtom>>())
    }

    fn foreign_send_counters_get(&self, block_id: &BlockId) -> Result<ForeignSendCounters, StorageError> {
        let key_prefix = ForeignSendCounterModel::key_prefix_by_block_id(block_id);
        let value = ForeignSendCounterModel::get_first(&self.tx, "foreign_send_counters_get", Some(&key_prefix), Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No foreign send counter in database".to_string() })?;
        Ok(value.counters)
    }

    fn foreign_receive_counters_get(&self) -> Result<ForeignReceiveCounters, StorageError> {
        let value = ForeignReceiveCounterModel::get_first(&self.tx, "foreign_send_counters_get", None, Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No foreign receive counter in database".to_string() })?;
        Ok(value.counters)
    }

    fn transactions_get(&self, tx_id: &TransactionId) -> Result<TransactionRecord, StorageError> {
        let key = TransactionModel::key_from_transaction_id(tx_id);
        Ok(TransactionModel::get(&self.tx, "transactions_get", &key)?)
    }

    fn transactions_exists(&self, tx_id: &TransactionId) -> Result<bool, StorageError> {
        let key = TransactionModel::key_from_transaction_id(tx_id);
        Ok(TransactionModel::key_exists(&self.tx, "transactions_exists", &key)?)
    }

    fn transactions_get_any<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &self,
        tx_ids: I,
    ) -> Result<Vec<TransactionRecord>, StorageError> {
        let tx_ids = tx_ids.into_iter().collect();
        Ok(TransactionModel::get_any(&self.tx, "transactions_get_any", tx_ids)?)
    }

    fn transactions_get_paginated(
        &self,
        limit: u64,
        offset: u64,
        _asc_desc_created_at: Option<Ordering>,
    ) -> Result<Vec<TransactionRecord>, StorageError> {
        // This operation is implemented in a naive way, by manually looping all transactions in the database.
        // As this method is only used for testing, further RocksDb database optimizations are probably not worth it

        let mut transactions: Vec<TransactionRecord> =
            TransactionModel::multi_get(&self.tx, None, Ordering::Ascending)?
            .into_iter()
            .collect();

        // pagination
        transactions = transactions
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();

        Ok(transactions)
    }

    fn transaction_executions_get(
        &self,
        tx_id: &TransactionId,
        block: &BlockId,
    ) -> Result<BlockTransactionExecution, StorageError> {
        let key_prefix = BlockTransactionExecutionModel::key_prefix_by_transaction_and_block(tx_id, Some(block));
        let executions = BlockTransactionExecutionModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Descending)?;
        
        match executions.first() {
            Some(execution) => Ok(execution.transaction_execution.clone()),
            None => Err(StorageError::NotFound { item: "transaction_execution", key: format!("tx_id={}, block={}", tx_id, block) }),
        }
    }

    fn transaction_executions_get_pending_for_block(
        &self,
        tx_id: &TransactionId,
        from_block_id: &BlockId,
    ) -> Result<BlockTransactionExecution, StorageError> {
        let operation = "transaction_executions_get_pending_for_block";

        if !self.blocks_exists(from_block_id)? {
            return Err(StorageError::QueryError {
                reason: format!(
                    "transaction_executions_get_pending_for_block: Block {} does not exist",
                    from_block_id
                ),
            });
        }

        let commit_block = self.get_commit_block_id()?;
        let block_ids = self.get_block_ids_between(&commit_block, from_block_id)?;

        // get the most recent escution of the transaction for every block in the range
        let mut executions: Vec<BlockTransactionExecutionModelData> = vec![];
        for block_id in block_ids {
            let block_id = BlockId::new(FixedHash::from_hex(&block_id).unwrap());
            let key_prefix = BlockTransactionExecutionModel::key_prefix_by_transaction_and_block(tx_id, Some(&block_id));
            let block_executions = BlockTransactionExecutionModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Descending)?;
            if let Some(exec) = block_executions.first() {
                executions.push(exec.clone());
            }
        }
        // get the latest execution
        executions.sort_by(|a,b| b.created_at.cmp(&a.created_at));
        let execution = executions.first();

        if let Some(execution) = execution {
            return Ok(execution.transaction_execution.clone());
        }

        // Otherwise look for executions after the commit block
        let key_prefix = BlockTransactionExecutionModel::key_prefix_by_transaction_and_block(tx_id, None);
        let executions = BlockTransactionExecutionModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Descending)?;
        for execution in executions {
            let key = BlockModel::key_from_block_id(execution.transaction_execution.block_id());
            let block = BlockModel::get(&self.tx, operation, &key)?;
            if block.is_committed() {
                return Ok(execution.transaction_execution)
            }
        }

        // No execution found
        Err(StorageError::QueryError {
            reason: format!(
                "transaction_executions_get_pending_for_block: no execution found for transaction_id {}",
                tx_id
            ),
        })
    }

    fn blocks_get(&self, block_id: &BlockId) -> Result<Block, StorageError> {
        let key = BlockModel::key_from_block_id(block_id);
        Ok(BlockModel::get(&self.tx, "blocks_get", &key)?)
    }

    fn blocks_get_all_ids_by_height(&self, epoch: Epoch, height: NodeHeight) -> Result<Vec<BlockId>, StorageError> {
        type Cf = crate::model::block::EpochHeightColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::build_key_prefix(epoch, Some(height));

        let block_ids =
            BlockModel::multi_get_ids_by_cf(self.db.clone(), &self.tx, "blocks_get_all_ids_by_height",  cf, &key_prefix)?
                .into_iter()
                .collect();

        Ok(block_ids)
    }

    fn blocks_get_genesis_for_epoch(&self, epoch: Epoch) -> Result<Block, StorageError> {
        type Cf = crate::model::block::EpochHeightColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::build_key_prefix(epoch, Some(NodeHeight(0)));

        let block_id =
            BlockModel::multi_get_ids_by_cf(self.db.clone(), &self.tx, "blocks_get_genesis_for_epoch",  cf, &key_prefix)?
            .into_iter()
            .next();

        if let Some(block_id) = block_id {
            let key = BlockModel::key_from_block_id(&block_id);
            let block= BlockModel::get(&self.tx, "blocks_get_genesis_for_epoch", &key)?;
            Ok(block)
        } else {
            Err(RocksDbStorageError::GeneralError { message: "Genesis block not found".to_owned() }.into())
        }
    }

    fn blocks_get_last_n_in_epoch(&self, n: usize, epoch: Epoch) -> Result<Vec<Block>, StorageError> {
        // TODO: this could be optimized by a new column familiy with the height reversed
        //       so we could avoid fetching the ids for all the blocks in the epoch

        type Cf = crate::model::block::EpochHeightColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::build_key_prefix(epoch, None);

        let block_ids: Vec<BlockId> =
            BlockModel::multi_get_ids_by_cf(self.db.clone(), &self.tx, "blocks_get_last_n_in_epoch",  cf, &key_prefix)?
            .into_iter()
            .collect();

        let mut blocks = vec![];
        for block_id in block_ids {
            let key = BlockModel::key_from_block_id(&block_id);
            let block= BlockModel::get(&self.tx, "blocks_get_last_n_in_epoch", &key)?;
            if block.is_committed() {
                blocks.push(block);
            }
        }
        // order by descending height 
        blocks.sort_by(|a, b| b.height().cmp(&a.height()));
        
        let last_n = blocks.into_iter().take(n).collect();

        Ok(last_n)
    }

    fn blocks_get_all_between(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
        start_block_height: NodeHeight,
        end_block_height: NodeHeight,
        include_dummy_blocks: bool,
        limit: u64,
    ) -> Result<Vec<Block>, StorageError> {
        // TODO: this operation could be optimized by creating a new column family that includes shard_group as part of the key

        let operation = "blocks_get_all_between";
        
        if start_block_height > end_block_height {
            return Err(StorageError::QueryError {
                reason: format!(
                    "Start block height {start_block_height} must be less than end block height {end_block_height}"
                ),
            });
        }

        type Cf = crate::model::block::EpochHeightColumnFamily;
        let cf = Cf::name();
        let lower_prefix = Cf::build_key_prefix(epoch, Some(start_block_height));
        // in rocksdb, the upper bound of a range is not included, and we want the blocks with the end height
        let upper_prefix = Cf::build_key_prefix(epoch, Some(end_block_height + NodeHeight(1)));

        let block_ids: Vec<BlockId> =
            BlockModel::multi_get_ids_by_cf_range(self.db.clone(), &self.tx, operation, cf, &lower_prefix, &upper_prefix)?
            .into_iter()
            .collect();

        let mut blocks = vec![];
        for block_id in block_ids {
            let key = BlockModel::key_from_block_id(&block_id);
            let block= BlockModel::get(&self.tx, operation, &key)?;

            if !include_dummy_blocks && block.is_dummy() {
                continue;
            }

            if block.shard_group() == shard_group {
                blocks.push(block);
            }
        }
        // order by ascending height 
        blocks.sort_by(|a, b| a.height().cmp(&b.height()));
        
        let first_n = blocks.into_iter().take(limit.try_into().unwrap()).collect();

        Ok(first_n)
    }

    fn blocks_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        let key = BlockModel::key_from_block_id(block_id);
        Ok(BlockModel::key_exists(&self.tx, "blocks_exists", &key)?)
    }

    fn blocks_is_ancestor(&self, descendant: &BlockId, ancestor: &BlockId) -> Result<bool, StorageError> {
        if !self.blocks_exists(descendant)? {
            return Err(StorageError::QueryError {
                reason: format!("blocks_is_ancestor: descendant block {} does not exist", descendant),
            });
        }

        if !self.blocks_exists(ancestor)? {
            return Err(StorageError::QueryError {
                reason: format!("blocks_is_ancestor: ancestor block {} does not exist", ancestor),
            });
        }

        // TODO: could this be optimized in RocksDB?
        let mut block_id = *descendant;
        while block_id != BlockId::genesis() {
            let key = BlockModel::key_from_block_id(&block_id);
            let block = BlockModel::get(&self.tx, "blocks_is_ancestor", &key)?;
            
            if block.parent() == ancestor {
                return Ok(true);
            }

            block_id = *block.parent();
        }

        Ok(false)
    }

    fn blocks_get_ids_by_parent(&self, parent_id: &BlockId) -> Result<Vec<BlockId>, StorageError> {
        type Cf = crate::model::block::ParentIdColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::build_key_prefix(parent_id);
        
        let block_ids =
            BlockModel::multi_get_ids_by_cf(self.db.clone(), &self.tx, "blocks_get_ids_by_parent",  cf, &key_prefix)?
            .into_iter()
            // Exclude the genesis block
            .filter(|block_id| block_id != parent_id)
            .collect();

        Ok(block_ids)
    }

    fn blocks_get_all_by_parent(&self, parent_id: &BlockId) -> Result<Vec<Block>, StorageError> {
        // get all the block ids first
        let block_ids = self.blocks_get_ids_by_parent(parent_id)?;

        // fetch each child by id
        let mut blocks = vec![];
        for block_id in block_ids {
            let key = BlockModel::key_from_block_id(&block_id);
            let block = BlockModel::get(&self.tx, "blocks_get_all_by_parent", &key)?;
            blocks.push(block);
        }
        
        Ok(blocks)
    }

    fn blocks_get_parent_chain(&self, block_id: &BlockId, limit: usize) -> Result<Vec<Block>, StorageError> {
        if !self.blocks_exists(block_id)? {
            return Err(StorageError::QueryError {
                reason: format!("blocks_get_parent_chain: descendant block {} does not exist", block_id),
            });
        }

        let mut blocks = vec![];
        let mut i = 0;
        let key = BlockModel::key_from_block_id(block_id);
        let initial_block = BlockModel::get(&self.tx, "blocks_is_ancestor", &key)?;  
        let mut current_block_id = *initial_block.parent();
        while i < limit && current_block_id != BlockId::genesis() {
            let key = BlockModel::key_from_block_id(&current_block_id);
            let block = BlockModel::get(&self.tx, "blocks_is_ancestor", &key)?;
            current_block_id = *block.parent();
            blocks.push(block);
            i += 1;
        }

        Ok(blocks)
    }

    fn blocks_get_pending_transactions(&self, block_id: &BlockId) -> Result<Vec<TransactionId>, StorageError> {
        type Cf = crate::model::missing_transactions::BlockIdColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::build_key_prefix_by_block(block_id);
        
        let transaction_ids =
            MissingTransactionModel::multi_get_cf(self.db.clone(), &self.tx, "blocks_get_pending_transactions",  cf, &key_prefix, Ordering::Ascending)?
            .into_iter()
            .map(|value| value.transaction_id)
            .collect();

        Ok(transaction_ids)
    }

    fn blocks_get_any_with_epoch_range(
        &self,
        epoch_range: RangeInclusive<Epoch>,
        validator_public_key: Option<&PublicKey>,
    ) -> Result<Vec<Block>, StorageError> {
        // TODO: this could be optimized by creating a new rocksdb column family for vn keys

        let operation = "blocks_get_any_with_epoch_range";

        type Cf = crate::model::block::EpochHeightColumnFamily;
        let cf = Cf::name();
        let lower_prefix = Cf::build_key_prefix(*epoch_range.start(), None);
        // in rocksdb, the upper bound of a range is not included, and we want the blocks with the end epoch
        let upper_prefix = Cf::build_key_prefix(epoch_range.end() + &Epoch(1), None);

        let block_ids: Vec<BlockId> =
            BlockModel::multi_get_ids_by_cf_range(self.db.clone(), &self.tx, operation,  cf, &lower_prefix, &upper_prefix)?
            .into_iter()
            .collect();

        let mut blocks = vec![];
        for block_id in block_ids {
            let key = BlockModel::key_from_block_id(&block_id);
            let block= BlockModel::get(&self.tx, "blocks_get_any_with_epoch_range", &key)?;

            if let Some(vn) = validator_public_key {
                if block.proposed_by() != vn {
                    continue
                }
            }

            blocks.push(block);
        }

        Ok(blocks)
    }

    fn blocks_get_paginated(
        &self,
        limit: u64,
        offset: u64,
        filter_index: Option<usize>,
        filter: Option<String>,
        ordering_index: Option<usize>,
        ordering: Option<Ordering>,
    ) -> Result<Vec<Block>, StorageError> {
        // This operation is implemented in a naive way, by manually looping all blocks in the database.
        // As this is only used for VN testing, further RocksDb database optimizations are probably not worth it

        let block_filter = |block: &Block| {
            let mut res = true;
            if let Some(filter) = &filter {
                if !filter.is_empty() {
                    if let Some(filter_index) = filter_index {
                        match filter_index {
                            0 => res = block.id().to_string().contains(filter),
                            1 => {
                                let epoch_number = filter.parse::<u64>().unwrap();
                                res = block.epoch() == Epoch(epoch_number);
                            },
                            2 => {
                                let height_number = filter.parse::<u64>().unwrap();
                                res = block.height() == NodeHeight(height_number);
                            },
                            4 => {
                                let cmd_number = filter.parse::<usize>().unwrap();
                                res = block.command_count() >= cmd_number;
                            },
                            5 => {
                                let fee = filter.parse::<u64>().unwrap();
                                res = block.total_leader_fee() >= fee;
                            },
                            7 => res = block.proposed_by().to_string().contains(filter),
                            _ => (),
                        }
                    } 
                }
            }
            res
        };

        // list all the blocks
        let mut blocks: Vec<Block> =
            BlockModel::multi_get(&self.tx, None, Ordering::Ascending)?
            .into_iter()
            .filter(block_filter)
            .collect();

        // ordering
        match ordering_index {
            Some(0) => blocks.sort_by(|a, b| a.id().cmp(b.id())),   
            Some(1) => blocks.sort_by(|a, b| a.epoch().cmp(&b.epoch())),
            Some(2) => blocks.sort_by(|a, b| (a.epoch(), a.height()).cmp(&(b.epoch(), b.height()))),
            Some(4) => blocks.sort_by(|a, b| a.command_count().cmp(&b.command_count())),
            Some(5) => blocks.sort_by(|a, b| a.total_leader_fee().cmp(&b.total_leader_fee())),
            Some(6) => blocks.sort_by(|a, b| a.block_time().cmp(&b.block_time())),
            // TODO: This filter is by creation time, but we don't have a created_at field yet in the corresponding RocksDB values
            Some(7) => (),
            Some(8) => blocks.sort_by(|a, b| a.proposed_by().cmp(&b.proposed_by())),
            _ => blocks.sort_by(|a, b| (a.epoch(), a.height()).cmp(&(b.epoch(), b.height()))),
        }

        if let Some(Ordering::Descending) = ordering {
            blocks.reverse();
        }

        // pagination
        blocks = blocks
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();

        Ok(blocks)
    }

    fn blocks_get_count(&self) -> Result<i64, StorageError> {
        let count = BlockModel::count(&self.tx, None)? as i64;
        Ok(count)
    }

    fn filtered_blocks_get_count(
        &self,
        filter_index: Option<usize>,
        filter: Option<String>,
    ) -> Result<i64, StorageError> {
        // TODO: this operation could be optimized by creating column families for all filtering fields

        let block_filter = |block: &Block| {
            let mut res = true;
            if let Some(filter) = &filter {
                if !filter.is_empty() {
                    if let Some(filter_index) = filter_index {
                        match filter_index {
                            0 => res = block.id().to_string().contains(filter),
                            1 => {
                                let epoch_number = filter.parse::<u64>().unwrap();
                                res = block.epoch() == Epoch(epoch_number);
                            },
                            2 => {
                                let height_number = filter.parse::<u64>().unwrap();
                                res = block.height() == NodeHeight(height_number);
                            },
                            4 => {
                                let cmd_number = filter.parse::<usize>().unwrap();
                                res = block.command_count() >= cmd_number;
                            },
                            5 => {
                                let fee = filter.parse::<u64>().unwrap();
                                res = block.total_leader_fee() >= fee;
                            },
                            7 => res = block.proposed_by().to_string().contains(filter),
                            _ => (),
                        }
                    } 
                }
            }
            res
        };

        let count = BlockModel::multi_get(&self.tx, None, Ordering::Ascending)?
            .into_iter()
            .filter(block_filter)
            .count() as i64;

        Ok(count)
    }

    fn blocks_max_height(&self) -> Result<NodeHeight, StorageError> {
        // This method is not used
        // TODO: remove it from the state store trait
        todo!()
    }

    fn block_diffs_get(&self, block_id: &BlockId) -> Result<BlockDiff, StorageError> {
        let key_prefix = BlockDiffModel::build_key_prefix(*block_id, None);
        let values = BlockDiffModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Ascending)?;

        Ok(BlockDiffData::load(*block_id, values))
    }

    fn block_diffs_get_last_change_for_substate(
        &self,
        block_id: &BlockId,
        substate_id: &SubstateId,
    ) -> Result<SubstateChange, StorageError> {
        let commit_block = self.get_commit_block_id()?;
        let block_ids = self.get_block_ids_with_commands_between(&commit_block, block_id)?;

        let mut diffs = vec![];
        for block_id in block_ids {
            let key_prefix = BlockDiffModel::build_key_prefix_str(&block_id, Some(substate_id));
            let values = BlockDiffModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Descending)?;
            diffs.extend(values);
        }

        // we want the most recent change
        diffs.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        let most_recent_diff = diffs.first()
            .ok_or_else(|| StorageError::General { details: "No block_diffs found".to_string() })?;
        Ok(most_recent_diff.change.clone())
    }

    fn quorum_certificates_get(&self, qc_id: &QcId) -> Result<QuorumCertificate, StorageError> {
        let key = QuorumCertificateModel::key_from_qc_id(qc_id);
        let qc = QuorumCertificateModel::get(&self.tx, "quorum_certificates_get", &key)?;
        Ok(qc)
    }

    fn quorum_certificates_get_all<'a, I: IntoIterator<Item = &'a QcId>>(
        &self,
        qc_ids: I,
    ) -> Result<Vec<QuorumCertificate>, StorageError> {
        let mut qcs = vec![];

        for qc_id in qc_ids {
            let qc = self.quorum_certificates_get(qc_id)?;
            qcs.push(qc);
        }

        Ok(qcs)
    }

    fn quorum_certificates_get_by_block_id(&self, block_id: &BlockId) -> Result<QuorumCertificate, StorageError> {
        let operation = "quorum_certificates_get_by_block_id";

        type Cf = crate::model::quorum_certificate::BlockColumnFamily;
        let cf = Cf::name();

        let key_prefix = Cf::key_from_block_id(block_id);
        let ordering = Ordering::Ascending;

        let res = QuorumCertificateModel::get_cf(self.db.clone(), &self.tx, cf, operation, Some(&key_prefix), ordering)?;

        let Some(qc) = res else {
            return Err(StorageError::NotFound { item: "quorum_certificate", key: format!("block_id={block_id}") });
        };

        Ok(qc)
    }

    fn transaction_pool_get_for_blocks(
        &self,
        from_block_id: &BlockId,
        to_block_id: &BlockId,
        transaction_id: &TransactionId,
    ) -> Result<TransactionPoolRecord, StorageError> {
        if !self.blocks_exists(from_block_id)? {
            return Err(StorageError::QueryError {
                reason: format!(
                    "transaction_pool_get_for_blocks: Block {} does not exist",
                    from_block_id
                ),
            });
        }

        if !self.blocks_exists(to_block_id)? {
            return Err(StorageError::QueryError {
                reason: format!("transaction_pool_get_for_blocks: Block {} does not exist", to_block_id),
            });
        }

        let mut updates = self.get_transaction_atom_state_updates_between_blocks(
            from_block_id,
            to_block_id,
            std::iter::once(transaction_id.to_string().as_str()),
        )?;

        debug!(
            target: LOG_TARGET,
            "transaction_pool_get: from_block_id={}, to_block_id={}, transaction_id={}, updates={} [{:?}]",
            from_block_id,
            to_block_id,
            transaction_id,
            updates.len(),
            updates.values().map(|v| v.block_id.clone()).collect::<Vec<_>>(),
        );

        let key = TransactionPoolModel::key_from_transaction_id(&transaction_id);
        let rec = TransactionPoolModel::get(&self.tx, "transaction_pool_get_for_blocks", &key)?;
        let rec = TransactionPoolModel::try_convert(&rec, updates.swap_remove(&transaction_id.to_string()))?;

        Ok(rec)
    }

    fn transaction_pool_exists(&self, transaction_id: &TransactionId) -> Result<bool, StorageError> {
        let key = TransactionPoolModel::key_from_transaction_id(transaction_id);
        let key_exists = TransactionPoolModel::key_exists(&self.tx, "transaction_pool_exists", &key)?;
        Ok(key_exists)
    }

    fn transaction_pool_get_all(&self) -> Result<Vec<TransactionPoolRecord>, StorageError> {
        let txs = TransactionPoolModel::multi_get(&self.tx, None, Ordering::Ascending)?;
        Ok(txs)
    }

    fn transaction_pool_get_many_ready(
        &self,
        max_txs: usize,
        block_id: &BlockId,
    ) -> Result<Vec<TransactionPoolRecord>, StorageError> {
        todo!()
        /*
        use crate::schema::{lock_conflicts, transaction_pool};

        if !self.blocks_exists(block_id)? {
            return Err(StorageError::QueryError {
                reason: format!("transaction_pool_get_many_ready: block {block_id} does not exist"),
            });
        }

        let mut ready_txs = transaction_pool::table
            // Exclude new transactions
            .filter(transaction_pool::stage.ne(TransactionPoolStage::New.to_string()))
            .filter(transaction_pool::is_ready.eq(true))
            .order_by(transaction_pool::transaction_id.asc())
            .get_results::<sql_models::TransactionPoolRecord>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "transaction_pool_get_many_ready",
                source: e,
            })?;

        debug!(
            target: LOG_TARGET,
            "🛢️ transaction_pool_get_many_ready: block_id={}, in progress ready_txs={}",
            block_id,
            ready_txs.len()
        );

        let new_limit = max_txs.saturating_sub(ready_txs.len());
        if new_limit > 0 {
            let new_txs = transaction_pool::table
                .filter(transaction_pool::stage.eq(TransactionPoolStage::New.to_string()))
                .filter(transaction_pool::is_ready.eq(true))
                // Filter out any transactions that are in lock conflict
                .filter(transaction_pool::transaction_id.ne_all(lock_conflicts::table.select(lock_conflicts::transaction_id)))
                .order_by(transaction_pool::transaction_id.asc())
                .limit(new_limit as i64)
                .get_results::<sql_models::TransactionPoolRecord>(self.connection())
                .map_err(|e| SqliteStorageError::DieselError {
                    operation: "transaction_pool_get_many_ready",
                    source: e,
                })?;

            debug!(
                target: LOG_TARGET,
                "🛢️ transaction_pool_get_many_ready: block_id={}, new ready_txs={}, total ready_txs={}",
                block_id,
                new_txs.len(),
                ready_txs.len() + new_txs.len()
            );
            ready_txs.extend(new_txs);
        }

        if ready_txs.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch all applicable block ids between the locked block and the given block
        let locked = self.get_current_locked_block()?;

        let mut updates = self.get_transaction_atom_state_updates_between_blocks(
            &locked.block_id,
            block_id,
            ready_txs.iter().map(|s| s.transaction_id.as_str()),
        )?;

        debug!(
            target: LOG_TARGET,
            "transaction_pool_get_many_ready: locked.block_id={}, leaf.block_id={}, len(ready_txs)={}, updates={}",
            locked.block_id,
            block_id,
            ready_txs.len(),
            updates.len()
        );

        ready_txs
            .into_iter()
            .map(|rec| {
                let maybe_update = updates.swap_remove(&rec.transaction_id);
                rec.try_convert(maybe_update)
            })
            // Filter only Ok where is_ready == true (after update) or Err
            .filter(|result| result.as_ref().map_or(true, |rec| rec.is_ready()))
            .take(max_txs)
            .collect()
            */
    }

    fn transaction_pool_count(
        &self,
        stage: Option<TransactionPoolStage>,
        is_ready: Option<bool>,
        confirmed_stage: Option<Option<TransactionPoolConfirmedStage>>,
        skip_lock_conflicted: bool,
    ) -> Result<usize, StorageError> {
        todo!()
        /*
        use crate::schema::transaction_pool;

        let mut query = transaction_pool::table.into_boxed();
        if let Some(stage) = stage {
            query = query.filter(
                transaction_pool::pending_stage
                    .eq(stage.to_string())
                    .or(transaction_pool::pending_stage
                        .is_null()
                        .and(transaction_pool::stage.eq(stage.to_string()))),
            );
        }
        if let Some(is_ready) = is_ready {
            query = query.filter(transaction_pool::is_ready.eq(is_ready));
        }

        match confirmed_stage {
            Some(Some(stage)) => {
                query = query.filter(transaction_pool::confirm_stage.eq(stage.to_string()));
            },
            Some(None) => {
                query = query.filter(transaction_pool::confirm_stage.is_null());
            },
            None => {},
        }

        let count =
            query
                .count()
                .get_result::<i64>(self.connection())
                .map_err(|e| SqliteStorageError::DieselError {
                    operation: "transaction_pool_count",
                    source: e,
                })?;

        Ok(count as usize)
        */
    }

    fn transactions_fetch_involved_shards(
        &self,
        transaction_ids: HashSet<TransactionId>,
    ) -> Result<HashSet<SubstateAddress>, StorageError> {
        todo!()
        /*
        use crate::schema::transactions;

        let tx_ids = transaction_ids.into_iter().map(serialize_hex).collect::<Vec<_>>();

        let inputs_per_tx = transactions::table
            .select(transactions::resolved_inputs)
            .filter(transactions::transaction_id.eq_any(&tx_ids))
            .load::<Option<String>>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "transaction_pools_fetch_involved_shards",
                source: e,
            })?;

        if inputs_per_tx.len() != tx_ids.len() {
            return Err(SqliteStorageError::NotAllItemsFound {
                items: "Transactions",
                operation: "transactions_fetch_involved_shards",
                details: format!(
                    "transactions_fetch_involved_shards: expected {} transactions, got {}",
                    tx_ids.len(),
                    inputs_per_tx.len()
                ),
            }
            .into());
        }

        let shards = inputs_per_tx
            .into_iter()
            .filter_map(|inputs| {
                // a Result is very inconvenient with flat_map
                inputs.map(|inputs| {
                    deserialize_json::<HashSet<SubstateAddress>>(&inputs)
                        .expect("[transactions_fetch_involved_shards] Failed to deserialize involved shards")
                })
            })
            .flatten()
            .collect();

        Ok(shards)
        */
    }

    fn votes_get_by_block_and_sender(
        &self,
        block_id: &BlockId,
        sender_leaf_hash: &FixedHash,
    ) -> Result<Vote, StorageError> {
        let key = VoteModel::key_from_block_and_sender(block_id, Some(sender_leaf_hash));
        let vote = VoteModel::get(&self.tx, "votes_get_by_block_and_sender", &key)?;
        Ok(vote)
    }

    fn votes_count_for_block(&self, block_id: &BlockId) -> Result<u64, StorageError> {
        let key_prefix = VoteModel::key_from_block_and_sender(block_id, None);
        let count = VoteModel::count(&self.tx, Some(&key_prefix))?;
        Ok(count)
    }

    fn votes_get_for_block(&self, block_id: &BlockId) -> Result<Vec<Vote>, StorageError> {
        let key_prefix = VoteModel::key_from_block_and_sender(block_id, None);
        let votes = VoteModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Descending)?;
        Ok(votes)
    }

    fn substates_get(&self, address: &SubstateAddress) -> Result<SubstateRecord, StorageError> {
        let key = SubstateModel::key_from_address(address);
        Ok(SubstateModel::get(&self.tx, "substates_get", &key)?)
    }

    fn substates_get_any<'a, I: IntoIterator<Item = &'a VersionedSubstateIdRef<'a>>>(
        &self,
        substate_ids: I,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        type Cf = crate::model::substate::VersionColumnFamily;

        let operation = "substates_get_any";
        // we want descending key order to get the highest version of each substate, because rocksdb orders incrementally by key
        let ordering = Ordering::Descending;

        let cf = Cf::name();
        let mut substates = vec![];

        for req in substate_ids {
            let requirement = SubstateRequirement::new(req.substate_id.clone(), Some(req.version));
            let key_prefix = Cf::build_key_from_requirement(&requirement);
            if let Some(substate) = SubstateModel::get_cf(self.db.clone(), &self.tx, cf, operation, Some(&key_prefix), ordering)? {
                substates.push(substate);
            }
        }

        return Ok(substates)
    }

    fn substates_get_any_max_version<'a, I: IntoIterator<Item = &'a SubstateId>>(
        &self,
        substate_ids: I,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        type Cf = crate::model::substate::VersionColumnFamily;

        let operation = "substates_get_any_max_version";
        let cf = Cf::name();
        // we want descending key order to get the highest version of each substate, because rocksdb orders incrementally by key
        let ordering = Ordering::Descending;

        let mut substates = vec![];

        for substate_id in substate_ids {
            let req = SubstateRequirement::new(substate_id.clone(), None);
            let key_prefix = Cf::build_key_from_requirement(&req);
            if let Some(substate) = SubstateModel::get_cf(self.db.clone(), &self.tx, cf, operation, Some(&key_prefix), ordering)? {
                substates.push(substate);
            }
        }

        return Ok(substates)
    }

    fn substates_get_max_version_for_substate(&self, substate_id: &SubstateId) -> Result<(u32, bool), StorageError> {
        type Cf = crate::model::substate::VersionColumnFamily;

        let operation = "substates_get_max_version_for_substate";
        let cf = Cf::name();
        // we want descending key order to get the highest version of the substate, because rocksdb orders incrementally by key
        let ordering = Ordering::Descending;

        let req = SubstateRequirement::new(substate_id.clone(), None);
        let key_prefix = Cf::build_key_from_requirement(&req);

        let res = SubstateModel::get_cf(self.db.clone(), &self.tx, cf, operation, Some(&key_prefix), ordering)?;

        match res {
            Some(substate) =>
                Ok((substate.version, substate.destroyed.is_some()))
            ,
            None => Err(StorageError::NotFound {
                item: "Substate (substates_get_max_version_for_substate)",
                key: substate_id.to_string(),
            })
        }
    }

    fn substates_any_exist<I: IntoIterator<Item = S>, S: Borrow<VersionedSubstateId>>(
        &self,
        addresses: I,
    ) -> Result<bool, StorageError> {
        let operation = "substates_any_exist";

        for address in addresses {
            let key = SubstateModel::key_from_address(&address.borrow().to_substate_address());
            let res = SubstateModel::get(&self.tx, operation, &key);
            match res {
                Ok(_) => return Ok(true),
                Err(e) => match e {
                    RocksDbStorageError::NotFound { .. } => continue,
                    _ => return Err(e.into()),
                }
            }
        }

        return Ok(false)
    }

    fn substates_exists_for_transaction(&self, transaction_id: &TransactionId) -> Result<bool, StorageError> {
        // This function is not used anywhere, so we skip implementation
        todo!()
    }

    fn substates_get_n_after(&self, n: usize, after: &SubstateAddress) -> Result<Vec<SubstateRecord>, StorageError> {
        // This function is not used anywhere, so we skip implementation
        todo!()
    }

    fn substates_get_many_within_range(
        &self,
        start: &SubstateAddress,
        end: &SubstateAddress,
        exclude: &[SubstateAddress],
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        // This function is not used anywhere, so we skip implementation
        todo!()
    }

    fn substates_get_many_by_created_transaction(
        &self,
        tx_id: &TransactionId,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        type Cf = crate::model::substate::CreatedByTxColumnFamily;

        let operation = "substates_get_many_by_created_transaction";
        let cf = Cf::name();
        let ordering = Ordering::Ascending; // order does not matter here
        let key_prefix = Cf::build_key_by_transaction(tx_id, None);

        let substates = SubstateModel::multi_get_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix, ordering)?;

        Ok(substates)
    }

    fn substates_get_many_by_destroyed_transaction(
        &self,
        tx_id: &TransactionId,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        type Cf = crate::model::substate::DestroyedByTxColumnFamily;

        let operation = "substates_get_many_by_destroyed_transaction";
        let cf = Cf::name();
        let ordering = Ordering::Ascending; // order does not matter here
        let key_prefix = Cf::build_key_by_transaction(tx_id, None);

        let substates = SubstateModel::multi_get_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix, ordering)?;

        Ok(substates)
    }

    fn substates_get_all_for_transaction(
        &self,
        tx_id: &TransactionId,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        let operation = "substates_get_all_for_transaction";
        let ordering = Ordering::Ascending;

        // get all created by transaction
        type CreatedCf = crate::model::substate::CreatedByTxColumnFamily;
        let cf = CreatedCf::name();
        let key_prefix = CreatedCf::build_key_by_transaction(tx_id, None);
        let created_by = SubstateModel::multi_get_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix, ordering)?;
        let mut substates = created_by
            .into_iter()
            .map(|s| (s.to_substate_address(), s))
            .collect::<HashMap<_,_>>();       
        
        // get all destroyed by transaction
        type DestroyedCf = crate::model::substate::DestroyedByTxColumnFamily;
        let cf = DestroyedCf::name();
        let key_prefix = DestroyedCf::build_key_by_transaction(tx_id, None);
        let destroyed_by = SubstateModel::multi_get_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix, ordering)?;
        for substate in destroyed_by {
            substates.insert(substate.to_substate_address(), substate);
        }    

        Ok(substates.into_values().collect())
    }

    fn substate_locks_get_locked_substates_for_transaction(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<Vec<LockedSubstateValue>, StorageError> {
        let operation = "substate_locks_get_locked_substates_for_transaction";

        type Cf = crate::model::substate_locks::TransactionIdColumnFamily;
        let key_prefix = Cf::build_key_prefix_by_transaction(transaction_id);
        let locks = SubstateLockModel::multi_get_cf(self.db.clone(), &self.tx, Cf::name(), operation, &key_prefix, Ordering::Ascending)?;
            
        let mut locked_substates = vec![];
        for lock in locks {
            let address = SubstateAddress::from_substate_id(&lock.substate_id, lock.lock.version());
            let key = SubstateModel::key_from_address(&address);
            if SubstateModel::key_exists(&self.tx, operation, &key)? {
                let substate = SubstateModel::get(&self.tx, operation, &key)?;

                let locked_substate = LockedSubstateValue {
                    substate_id: lock.substate_id,
                    lock: lock.lock,
                    value: substate.substate_value,
                };
                locked_substates.push(locked_substate);
            }
        }

        Ok(locked_substates)
    }

    fn substate_locks_get_latest_for_substate(&self, substate_id: &SubstateId) -> Result<SubstateLock, StorageError> {
        let key_prefix = SubstateLockModel::key_prefix_by_substate_id(substate_id);

        let lock = SubstateLockModel::
            get_first(&self.tx, "substate_locks_get_latest_for_substate", Some(&key_prefix), Ordering::Descending)?
            .ok_or_else(|| StorageError::General { details: "No locked substate found".to_string() })?;

        Ok(lock.lock)
    }

    fn pending_state_tree_diffs_get_all_up_to_commit_block(
        &self,
        block_id: &BlockId,
    ) -> Result<HashMap<Shard, Vec<PendingShardStateTreeDiff>>, StorageError> {
        if !self.blocks_exists(block_id)? {
            return Err(StorageError::NotFound {
                item: "pending_state_tree_diffs_get_all_up_to_commit_block: Block",
                key: block_id.to_string(),
            });
        }
        
        // Get the last committed block
        let committed_block_id = self.get_commit_block_id()?;
        
        // Block may modify state with zero commands because the justify a block that changes state
        let block_ids = self.get_block_ids_between(&committed_block_id, block_id)?;

        if block_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut diff_recs = vec![];
        for block_id in block_ids {
            let key_prefix = PendingStateTreeDiffModel::key_from_block_str_and_height(&block_id, None);
            let diffs = PendingStateTreeDiffModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Ascending)?;
            diff_recs.extend(diffs);
        }

        let mut diffs = HashMap::new();
        for diff in diff_recs {
            let shard = diff.shard;
            let diff = PendingShardStateTreeDiff::from(diff);
            diffs
                .entry(shard)
                .or_insert_with(Vec::new) //PendingStateTreeDiff::default)
                .push(diff);
        }

        Ok(diffs)
    }

    fn state_transitions_get_n_after(
        &self,
        n: usize,
        id: StateTransitionId,
        end_epoch: Epoch,
    ) -> Result<Vec<StateTransition>, StorageError> {
        todo!()
        /*
        use crate::schema::{state_transitions, substates};

        debug!(target: LOG_TARGET, "state_transitions_get_n_after: {id}, end_epoch:{end_epoch}");

        let transitions = state_transitions::table
            .left_join(substates::table.on(state_transitions::substate_address.eq(substates::address)))
            .select((state_transitions::all_columns, substates::all_columns.nullable()))
            .filter(state_transitions::seq.gt(id.seq() as i64))
            .filter(state_transitions::shard.eq(id.shard().as_u32() as i32))
            .filter(state_transitions::epoch.lt(end_epoch.as_u64() as i64))
            .limit(n as i64)
            .get_results::<(sql_models::StateTransition, Option<sql_models::SubstateRecord>)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "state_transitions_get_n_after",
                source: e,
            })?;

        transitions
            .into_iter()
            .map(|(t, s)| {
                let s = s.ok_or_else(|| StorageError::DataInconsistency {
                    details: format!("substate entry does not exist for transition {}", t.id),
                })?;

                t.try_convert(s)
            })
            .collect()
            */
    }

    fn state_transitions_get_last_id(&self, shard: Shard) -> Result<StateTransitionId, StorageError> {
        todo!()
        /*
        use crate::schema::state_transitions;

        let (seq, epoch) = state_transitions::table
            .select((state_transitions::seq, state_transitions::epoch))
            .filter(state_transitions::shard.eq(shard.as_u32() as i32))
            .order_by(state_transitions::epoch.desc())
            .then_order_by(state_transitions::seq.desc())
            .first::<(i64, i64)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "state_transitions_get_last_id",
                source: e,
            })?;

        let epoch = Epoch(epoch as u64);
        let seq = seq as u64;

        Ok(StateTransitionId::new(epoch, shard, seq))
        */
    }

    fn state_tree_nodes_get(&self, shard: Shard, key: &NodeKey) -> Result<Node<Version>, StorageError> {
        let operation = "state_tree_nodes_get";
        let key = StateTreeModel::key_from_shard_and_node(&shard, key);
        let value = StateTreeModel::get(&self.tx, operation, &key)?;

        Ok(value.node)
    }

    fn state_tree_versions_get_latest(&self, shard: Shard) -> Result<Option<Version>, StorageError> {
        let operation = "state_tree_versions_get_latest";
        let key = StateTreeShardVersionModel::key_from_shard(&shard);

        if !StateTreeShardVersionModel::key_exists(&self.tx, operation, &key)? {
            return Ok(None)
        }

        let value = StateTreeShardVersionModel::get(&self.tx, operation, &key)?;
        Ok(Some(value.version))
    }

    fn epoch_checkpoint_get(&self, epoch: Epoch) -> Result<EpochCheckpoint, StorageError> {
        let operation = "epoch_checkpoint_get";
        let key = EpochCheckpointModel::key_from_epoch(&epoch);
        let value = EpochCheckpointModel::get(&self.tx, operation, &key)?;
        Ok(value)
    }

    fn foreign_substate_pledges_get_all_by_transaction_id(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<SubstatePledges, StorageError> {
        let key_prefix = ForeignSubstatePledgeModel::key_from_transaction_and_address(transaction_id, None);
        let foreign_pledges = ForeignSubstatePledgeModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Ascending)?;
        let substate_pledges = foreign_pledges
            .into_iter()
            .map(|p| p.pledge)
            .collect();
        
        Ok(substate_pledges)
    }

    fn burnt_utxos_get(&self, commitment: &UnclaimedConfidentialOutputAddress) -> Result<BurntUtxo, StorageError> {
        let operation = "burnt_utxos_get";
        let key = BurntUtxoModel::key_from_commitment(commitment);
        let value = BurntUtxoModel::get(&self.tx, operation, &key)?;
        Ok(value)
    }

    fn burnt_utxos_get_all_unproposed(
        &self,
        leaf_block: &BlockId,
        limit: usize,
    ) -> Result<Vec<BurntUtxo>, StorageError> {
        if !self.blocks_exists(leaf_block)? {
            return Err(StorageError::NotFound {
                item: "Block",
                key: leaf_block.to_string(),
            });
        }

        if limit == 0 {
            return Ok(Vec::new());
        }

        let locked_block = self.get_current_locked_block()?;
        let exclude_block_ids = self.get_block_ids_with_commands_between(&locked_block.block_id, leaf_block)?;

        // TODO: optimize this query in RocksDB
        // TODO: implement limit in RocksDB model
        let utxos = BurntUtxoModel::multi_get( &self.tx, None, Ordering::Ascending)?;
        Ok(utxos
            .into_iter()
            .filter(|u| {
                let Some(proposed_in_block) = u.proposed_in_block else {
                    return true;
                };

                let is_excluded = exclude_block_ids.contains(&proposed_in_block.to_string());

                !is_excluded
            })
            .collect())
    }

    fn burnt_utxos_count(&self) -> Result<u64, StorageError> {
        let count = BurntUtxoModel::count(&self.tx, None)?;
        Ok(count)
    }

    fn foreign_parked_blocks_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        let key = ForeignParkedBlockModel::key_from_block_id(block_id);
        let block_exists = ForeignParkedBlockModel::key_exists(&self.tx, "foreign_parked_blocks_exists", &key)?;
        Ok(block_exists)
    }

    fn validator_epoch_stats_get(
        &self,
        epoch: Epoch,
        public_key: &PublicKey,
    ) -> Result<ValidatorConsensusStats, StorageError> {
        todo!()
        /*
        use crate::schema::validator_epoch_stats;

        let (participation_shares, missed_proposals) = validator_epoch_stats::table
            .select((
                validator_epoch_stats::participation_shares,
                validator_epoch_stats::missed_proposals,
            ))
            .filter(validator_epoch_stats::public_key.eq(public_key.to_hex()))
            .filter(validator_epoch_stats::epoch.eq(epoch.as_u64() as i64))
            .get_result::<(i64, i64)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "validator_epoch_stats_get",
                source: e,
            })?;

        Ok(ValidatorConsensusStats {
            missed_proposals: missed_proposals
                .try_into()
                .map_err(|_| StorageError::DataInconsistency {
                    details: "validator_epoch_stats_get: missed_proposals is negative".to_string(),
                })?,
            participation_shares: participation_shares
                .try_into()
                .map_err(|_| StorageError::DataInconsistency {
                    details: "validator_epoch_stats_get: participation_shares is negative".to_string(),
                })?,
        })
        */
    }
    
    fn validator_epoch_stats_get_nodes_to_evict(
        &self,
        block_id: &BlockId,
        threshold: u64,
        limit: u64,
    ) -> Result<Vec<PublicKey>, StorageError> {
        todo!()
    }
    
    fn suspended_nodes_is_evicted(&self, block_id: &BlockId, public_key: &PublicKey) -> Result<bool, StorageError> {
        if !self.blocks_exists(block_id)? {
            return Err(StorageError::QueryError {
                reason: format!("block {} not found", block_id),
            });
        }

        let operation = "suspended_nodes_is_evicted";

        let commit_block_id = self.get_commit_block_id()?;
        let block_key = BlockModel::key_from_block_id(&commit_block_id);
        let commit_block = BlockModel::get(&self.tx, operation, &block_key)?;

        let block_ids = self.get_block_ids_between(&commit_block_id, block_id)?;

        let key_prefix = EvictedNodeModel::key_prefix_by_public_key(public_key);
        let count =
            EvictedNodeModel::multi_get(&self.tx, Some(&key_prefix), Ordering::Ascending)?
            .into_iter()
            .filter(|n|
                !block_ids.contains(&n.evicted_in_block.to_string()) || n.evicted_in_block_height <= commit_block.height()
            )
            .count();

        Ok(count > 0)
    }
    
    fn evicted_nodes_count(&self, epoch: Epoch) -> Result<u64, StorageError> {
        type Cf = crate::model::evicted_node::EvictionCommittedColumnFamily;
        let key_prefix = Cf::key_prefix_by_epoch(&epoch);
        let count = MissingTransactionModel::count_cf(self.db.clone(), &self.tx, Cf::name(), Some(&key_prefix))?;

        Ok(count)
    }
    
    fn transaction_pool_has_pending_state_updates(&self, _block_id: &BlockId) -> Result<bool, StorageError> {
        todo!()
    }
    
    fn block_diffs_get_change_for_versioned_substate<'a, T: Into<VersionedSubstateIdRef<'a>>>(
        &self,
        block_id: &BlockId,
        substate_id: T,
    ) -> Result<SubstateChange, StorageError> {
        todo!()
    }
    
    fn substate_locks_has_any_write_locks_for_substates<'a, I: IntoIterator<Item = &'a SubstateId>>(
        &self,
        exclude_transaction_id: Option<&TransactionId>,
        substate_ids: I,
        exclude_local_only: bool,
    ) -> Result<Option<TransactionId>, StorageError> {
        todo!()
    }
    
    fn foreign_substate_pledges_exists_for_transaction_and_address<T: ToSubstateAddress>(
        &self,
        transaction_id: &TransactionId,
        address: T,
    ) -> Result<bool, StorageError> {
        todo!()
    }
    
    fn foreign_substate_pledges_get_write_pledges_to_transaction<'a, I: IntoIterator<Item = &'a SubstateId>>(
        &self,
        transaction_id: &TransactionId,
        substate_ids: I,
    ) -> Result<SubstatePledges, StorageError> {
        todo!()
    }
}
