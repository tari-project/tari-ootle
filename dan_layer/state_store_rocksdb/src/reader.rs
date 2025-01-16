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
    VersionedSubstateId,
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

use crate::{error::RocksDbStorageError, model::{self, block::BlockModel, block_transaction_execution::{BlockTransactionExecutionModel, BlockTransactionExecutionModelData}, model::{ModelColumnFamily, RocksdbModel}, state_tree_shard_versions::StateTreeShardVersionModel, substate::SubstateModel, transaction::TransactionModel, transaction_pool::TransactionPoolModel, transaction_pool_state_update::{TransactionPoolStateUpdateModel, TransactionPoolStateUpdateModelData}}};

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
        while block_id != *start_block || block_id != BlockId::genesis() {
            let key: String = BlockModel::key_from_block_id(&block_id);
            let block = BlockModel::get(&self.tx, "blocks_parent_id", &key)?;
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
        /*
        use crate::schema::transactions;

        let count = transactions::table
            .count()
            .get_result::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "transactions_count",
                source: e,
            })?;

        Ok(count as u64)
        */
    }

    pub(crate) fn get_commit_block_id(&self) -> Result<BlockId, StorageError> {
        let block_opt = BlockModel::get_cf(self.db.clone(), &self.tx, model::block::IsCommittedColumnFamily::NAME, "get_commit_block_id", "", Ordering::Descending)?;
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
        todo!()
        /*
        use crate::schema::locked_block;

        let locked_block = locked_block::table
            .order_by(locked_block::id.desc())
            .first::<sql_models::LockedBlock>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "get_current_locked_block",
                source: e,
            })?;

        locked_block.try_into()
        */
    }
}

impl<'tx, TAddr: NodeAddressable + Serialize + DeserializeOwned + 'tx> StateStoreReadTransaction
    for RocksDbStateStoreReadTransaction<'tx, TAddr>
{
    type Addr = TAddr;

    fn last_sent_vote_get(&self) -> Result<LastSentVote, StorageError> {
        todo!()
        /*
        use crate::schema::last_sent_vote;

        let last_voted = last_sent_vote::table
            .order_by(last_sent_vote::id.desc())
            .first::<sql_models::LastSentVote>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "last_sent_vote_get",
                source: e,
            })?;

        last_voted.try_into()
        */
    }

    fn last_voted_get(&self) -> Result<LastVoted, StorageError> {
        todo!()
        /*
        use crate::schema::last_voted;

        let last_voted = last_voted::table
            .order_by(last_voted::id.desc())
            .first::<sql_models::LastVoted>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "last_voted_get",
                source: e,
            })?;

        last_voted.try_into()
        */
    }

    fn last_executed_get(&self) -> Result<LastExecuted, StorageError> {
        todo!()
        /*
        use crate::schema::last_executed;

        let last_executed = last_executed::table
            .order_by(last_executed::id.desc())
            .first::<sql_models::LastExecuted>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "last_executed_get",
                source: e,
            })?;

        last_executed.try_into()
        */
    }

    fn last_proposed_get(&self) -> Result<LastProposed, StorageError> {
        todo!()
        /*
        use crate::schema::last_proposed;

        let last_proposed = last_proposed::table
            .order_by(last_proposed::id.desc())
            .first::<sql_models::LastProposed>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "last_proposed_get",
                source: e,
            })?;

        last_proposed.try_into()
        */
    }

    fn locked_block_get(&self, epoch: Epoch) -> Result<LockedBlock, StorageError> {
        todo!()
        /*
        use crate::schema::locked_block;

        let locked_block = locked_block::table
            .filter(locked_block::epoch.eq(epoch.as_u64() as i64))
            .order_by(locked_block::id.desc())
            .first::<sql_models::LockedBlock>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "locked_block_get",
                source: e,
            })?;

        locked_block.try_into()
        */
    }

    fn leaf_block_get(&self, epoch: Epoch) -> Result<LeafBlock, StorageError> {
        todo!()
        /*
        use crate::schema::leaf_blocks;

        let leaf_block = leaf_blocks::table
            .filter(leaf_blocks::epoch.eq(epoch.as_u64() as i64))
            .order_by(leaf_blocks::id.desc())
            .first::<sql_models::LeafBlock>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "leaf_block_get",
                source: e,
            })?;

        leaf_block.try_into()
        */
    }

    fn high_qc_get(&self, epoch: Epoch) -> Result<HighQc, StorageError> {
        todo!()
        /*
        use crate::schema::high_qcs;

        let high_qc = high_qcs::table
            .filter(high_qcs::epoch.eq(epoch.as_u64() as i64))
            .order_by(high_qcs::id.desc())
            .first::<sql_models::HighQc>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "high_qc_get",
                source: e,
            })?;

        high_qc.try_into()
        */
    }

    fn foreign_proposals_get_any<'a, I: IntoIterator<Item = &'a BlockId>>(
        &self,
        block_ids: I,
    ) -> Result<Vec<ForeignProposal>, StorageError> {
        todo!()
        /*
        use crate::schema::{foreign_proposals, quorum_certificates};

        let foreign_proposals = foreign_proposals::table
            .left_join(quorum_certificates::table.on(foreign_proposals::justify_qc_id.eq(quorum_certificates::qc_id)))
            .filter(foreign_proposals::block_id.eq_any(block_ids.into_iter().map(serialize_hex)))
            .get_results::<(sql_models::ForeignProposal, Option<sql_models::QuorumCertificate>)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_proposals_get_any",
                source: e,
            })?;

        foreign_proposals
            .into_iter()
            .map(|(proposal, qc)| {
                let justify_qc = qc.ok_or_else(|| SqliteStorageError::DbInconsistency {
                    operation: "foreign_proposals_get_any",
                    details: format!(
                        "foreign proposal {} references non-existent quorum certificate {}",
                        proposal.block_id, proposal.justify_qc_id
                    ),
                })?;
                proposal.try_convert(justify_qc)
            })
            .collect()
            */
    }

    fn foreign_proposals_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_proposals;

        let foreign_proposals = foreign_proposals::table
            .filter(foreign_proposals::block_id.eq(serialize_hex(block_id)))
            .count()
            .limit(1)
            .get_result::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_proposals_exists",
                source: e,
            })?;

        Ok(foreign_proposals > 0)
        */
    }

    fn foreign_proposals_has_unconfirmed(&self, epoch: Epoch) -> Result<bool, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_proposals;

        let foreign_proposals = foreign_proposals::table
            .filter(foreign_proposals::epoch.eq(epoch.as_u64() as i64))
            .filter(foreign_proposals::status.ne(ForeignProposalStatus::Confirmed.to_string()))
            .count()
            .limit(1)
            .get_result::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_proposals_has_unconfirmed",
                source: e,
            })?;

        Ok(foreign_proposals > 0)
        */
    }

    fn foreign_proposals_get_all_new(
        &self,
        block_id: &BlockId,
        limit: usize,
    ) -> Result<Vec<ForeignProposal>, StorageError> {
        todo!()
        /*
        use crate::schema::{foreign_proposals, quorum_certificates};

        if !self.blocks_exists(block_id)? {
            return Err(StorageError::NotFound {
                item: "foreign_proposals_get_all_new: Block".to_string(),
                key: block_id.to_string(),
            });
        }

        let locked = self.get_current_locked_block()?;
        let pending_block_ids = self.get_block_ids_with_commands_between(&locked.block_id, block_id)?;

        let foreign_proposals = foreign_proposals::table
            .left_join(quorum_certificates::table.on(foreign_proposals::justify_qc_id.eq(quorum_certificates::qc_id)))
            .filter(foreign_proposals::epoch.eq(locked.epoch.as_u64() as i64))
            .filter(
                foreign_proposals::proposed_in_block
                    .is_null()
                    .or(foreign_proposals::proposed_in_block
                        .ne_all(pending_block_ids)
                        .and(foreign_proposals::proposed_in_block_height.gt(locked.height.as_u64() as i64))),
            )
            .limit(i64::try_from(limit).unwrap_or(i64::MAX))
            .get_results::<(sql_models::ForeignProposal, Option<sql_models::QuorumCertificate>)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_proposals_get_all_new",
                source: e,
            })?;

        foreign_proposals
            .into_iter()
            .map(|(proposal, qc)| {
                let justify_qc = qc.ok_or_else(|| SqliteStorageError::DbInconsistency {
                    operation: "foreign_proposals_get_all_new",
                    details: format!(
                        "foreign proposal {} references non-existent quorum certificate {}",
                        proposal.block_id, proposal.justify_qc_id
                    ),
                })?;
                proposal.try_convert(justify_qc)
            })
            .collect()
            */
    }

    fn foreign_proposal_get_all_pending(
        &self,
        from_block_id: &BlockId,
        to_block_id: &BlockId,
    ) -> Result<Vec<ForeignProposalAtom>, StorageError> {
        todo!()
        /*
        use crate::schema::blocks;

        let blocks = self.get_block_ids_with_commands_between(from_block_id, to_block_id)?;

        let all_commands: Vec<String> = blocks::table
            .select(blocks::commands)
            .filter(blocks::command_count.gt(0)) // if there is no command, then there is definitely no foreign proposal command
            .filter(blocks::block_id.eq_any(blocks))
            .load::<String>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_proposal_get_all",
                source: e,
            })?;
        let all_commands = all_commands
            .into_iter()
            .map(|commands| deserialize_json(commands.as_str()))
            .collect::<Result<Vec<Vec<Command>>, _>>()?;
        let all_commands = all_commands.into_iter().flatten().collect::<Vec<_>>();
        Ok(all_commands
            .into_iter()
            .filter_map(|command| command.foreign_proposal().cloned())
            .collect::<Vec<ForeignProposalAtom>>())
            */
    }

    fn foreign_send_counters_get(&self, block_id: &BlockId) -> Result<ForeignSendCounters, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_send_counters;

        let counter = foreign_send_counters::table
            .filter(foreign_send_counters::block_id.eq(serialize_hex(block_id)))
            .first::<sql_models::ForeignSendCounters>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_send_counters_get",
                source: e,
            })?;

        counter.try_into()
        */
    }

    fn foreign_receive_counters_get(&self) -> Result<ForeignReceiveCounters, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_receive_counters;

        let counter = foreign_receive_counters::table
            .order_by(foreign_receive_counters::id.desc())
            .first::<sql_models::ForeignReceiveCounters>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_receive_counters_get",
                source: e,
            })?;

        counter.try_into()
        */
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
        todo!()
        /*
        use crate::schema::missing_transactions;

        let txs = missing_transactions::table
            .select(missing_transactions::transaction_id)
            .filter(missing_transactions::block_id.eq(serialize_hex(block_id)))
            .get_results::<String>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "blocks_get_missing_transactions",
                source: e,
            })?;
        txs.into_iter().map(|s| deserialize_hex_try_from(&s)).collect()
        */
    }

    fn blocks_get_total_leader_fee_for_epoch(
        &self,
        epoch: Epoch,
        validator_public_key: &PublicKey,
    ) -> Result<u64, StorageError> {
        // TODO: to optimize this query we could create a new column familiy with epoch and proposed_by fields in the key

        let operation = "blocks_get_total_leader_fee_for_epoch";

        type Cf = crate::model::block::EpochHeightColumnFamily;
        let cf = Cf::name();
        let key_prefix = Cf::build_key_prefix(epoch, None);

        let block_ids = BlockModel::multi_get_ids_by_cf(self.db.clone(), &self.tx, operation, cf, &key_prefix)?;

        let mut sum = 0;
        for block_id in block_ids {
            let key = BlockModel::key_from_block_id(&block_id);
            let block= BlockModel::get(&self.tx, operation, &key)?;

            if block.proposed_by() == validator_public_key {
                sum += block.total_leader_fee();
            }
        }

        Ok(sum)
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
        todo!()
        /*
        use crate::schema::block_diffs;

        let block_diff = block_diffs::table
            .filter(block_diffs::block_id.eq(serialize_hex(block_id)))
            .get_results::<sql_models::BlockDiff>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "block_diffs_get",
                source: e,
            })?;

        sql_models::BlockDiff::try_load(*block_id, block_diff)
        */
    }

    fn block_diffs_get_last_change_for_substate(
        &self,
        block_id: &BlockId,
        substate_id: &SubstateId,
    ) -> Result<SubstateChange, StorageError> {
        todo!()
        /*
        use crate::schema::block_diffs;
        let commit_block = self.get_commit_block_id()?;
        let block_ids = self.get_block_ids_with_commands_between(&commit_block, block_id)?;

        let diff = block_diffs::table
            .filter(block_diffs::block_id.eq_any(block_ids))
            .filter(block_diffs::substate_id.eq(substate_id.to_string()))
            .order_by(block_diffs::id.desc())
            .first::<sql_models::BlockDiff>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "block_diffs_get_last_change_for_substate",
                source: e,
            })?;

        sql_models::BlockDiff::try_convert_change(diff)
        */
    }

    fn quorum_certificates_get(&self, qc_id: &QcId) -> Result<QuorumCertificate, StorageError> {
        todo!()
        /*
        use crate::schema::quorum_certificates;

        let qc_json = quorum_certificates::table
            .select(quorum_certificates::json)
            .filter(quorum_certificates::qc_id.eq(serialize_hex(qc_id)))
            .first::<String>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "quorum_certificates_get",
                source: e,
            })?;

        deserialize_json(&qc_json)
        */
    }

    fn quorum_certificates_get_all<'a, I: IntoIterator<Item = &'a QcId>>(
        &self,
        qc_ids: I,
    ) -> Result<Vec<QuorumCertificate>, StorageError> {
        todo!()
        /*
        use crate::schema::quorum_certificates;

        let qc_ids: Vec<String> = qc_ids.into_iter().map(serialize_hex).collect();

        let qc_json = quorum_certificates::table
            .select(quorum_certificates::json)
            .filter(quorum_certificates::qc_id.eq_any(&qc_ids))
            .get_results::<String>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "quorum_certificates_get_all",
                source: e,
            })?;

        if qc_json.len() != qc_ids.len() {
            return Err(SqliteStorageError::NotAllItemsFound {
                items: "QCs",
                operation: "quorum_certificates_get_all",
                details: format!(
                    "quorum_certificates_get_all: expected {} quorum certificates, got {}",
                    qc_ids.len(),
                    qc_json.len()
                ),
            }
            .into());
        }

        qc_json.iter().map(|j| deserialize_json(j)).collect()
        */
    }

    fn quorum_certificates_get_by_block_id(&self, block_id: &BlockId) -> Result<QuorumCertificate, StorageError> {
        todo!()
        /*
        use crate::schema::quorum_certificates;

        let qc_json = quorum_certificates::table
            .select(quorum_certificates::json)
            .filter(quorum_certificates::block_id.eq(serialize_hex(block_id)))
            .first::<String>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "quorum_certificates_get_by_block_id",
                source: e,
            })?;

        deserialize_json(&qc_json)
        */
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
        todo!()
        /*
        use crate::schema::transaction_pool;

        let count = transaction_pool::table
            .count()
            .filter(transaction_pool::transaction_id.eq(serialize_hex(transaction_id)))
            .first::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "transaction_pool_exists",
                source: e,
            })?;

        Ok(count > 0)
        */
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
        todo!()
        /*
        use crate::schema::votes;

        let vote = votes::table
            .filter(votes::block_id.eq(serialize_hex(block_id)))
            .filter(votes::sender_leaf_hash.eq(serialize_hex(sender_leaf_hash)))
            .first::<sql_models::Vote>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "votes_get",
                source: e,
            })?;

        Vote::try_from(vote)
        */
    }

    fn votes_count_for_block(&self, block_id: &BlockId) -> Result<u64, StorageError> {
        todo!()
        /*
        use crate::schema::votes;

        let count = votes::table
            .filter(votes::block_id.eq(serialize_hex(block_id)))
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "votes_count_for_block",
                source: e,
            })?;

        Ok(count as u64)
        */
    }

    fn votes_get_for_block(&self, block_id: &BlockId) -> Result<Vec<Vote>, StorageError> {
        todo!()
        /*
        use crate::schema::votes;

        let votes = votes::table
            .filter(votes::block_id.eq(serialize_hex(block_id)))
            .get_results::<sql_models::Vote>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "votes_get_for_block",
                source: e,
            })?;

        votes.into_iter().map(Vote::try_from).collect()
        */
    }

    fn substates_get(&self, address: &SubstateAddress) -> Result<SubstateRecord, StorageError> {
        let key = SubstateModel::key_from_address(address);
        Ok(SubstateModel::get(&self.tx, "substates_get", &key)?)
    }

    fn substates_get_any(
        &self,
        substate_ids: &HashSet<SubstateRequirement>,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        type Cf = crate::model::substate::VersionColumnFamily;

        let operation = "substates_get_any";
        // we want descending key order to get the highest version of each substate, because rocksdb orders incrementally by key
        let ordering = Ordering::Descending;

        let cf = Cf::name();
        let mut substates = vec![];

        for req in substate_ids {
            let key_prefix = Cf::build_key_from_requirement(req);
            if let Some(substate) = SubstateModel::get_cf(self.db.clone(), &self.tx, cf, operation, &key_prefix, ordering)? {
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
            if let Some(substate) = SubstateModel::get_cf(self.db.clone(), &self.tx, cf, operation, &key_prefix, ordering)? {
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

        let res = SubstateModel::get_cf(self.db.clone(), &self.tx, cf, operation, &key_prefix, ordering)?;

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
        todo!()
        /*
        use crate::schema::{substate_locks, substates};

        let recs = substate_locks::table
            .left_join(
                substates::table.on(substate_locks::substate_id
                    .eq(substates::substate_id)
                    .and(substate_locks::version.eq(substates::version))),
            )
            .filter(substate_locks::transaction_id.eq(serialize_hex(transaction_id)))
            .order_by(substate_locks::id.asc())
            .get_results::<(sql_models::SubstateLock, Option<sql_models::SubstateRecord>)>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "substate_locks_get_value_for_transaction",
                source: e,
            })?;

        recs.into_iter()
            .map(|(lock, maybe_substate)| lock.try_into_locked_substate_value(maybe_substate))
            .collect()
            */
    }

    fn substate_locks_get_latest_for_substate(&self, substate_id: &SubstateId) -> Result<SubstateLock, StorageError> {
        todo!()
        /*
        use crate::schema::substate_locks;

        // TODO: this may return an invalid lock if:
        // 1. the proposer links the parent block to the locked block instead of the previous tip
        // 2. if there are any inactive locks that were not removed from previous uncommitted blocks.

        let lock = substate_locks::table
            .filter(substate_locks::substate_id.eq(substate_id.to_string()))
            .order_by(substate_locks::id.desc())
            .first::<sql_models::SubstateLock>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "substate_locks_get_latest_for_substate",
                source: e,
            })?;

        lock.try_into_substate_lock()
        */
    }

    fn pending_state_tree_diffs_get_all_up_to_commit_block(
        &self,
        block_id: &BlockId,
    ) -> Result<HashMap<Shard, Vec<PendingShardStateTreeDiff>>, StorageError> {
        todo!()
        /*
        use crate::schema::pending_state_tree_diffs;

        if !self.blocks_exists(block_id)? {
            return Err(StorageError::NotFound {
                item: "pending_state_tree_diffs_get_all_up_to_commit_block: Block".to_string(),
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

        let diff_recs = pending_state_tree_diffs::table
            .filter(pending_state_tree_diffs::block_id.eq_any(block_ids))
            .order_by(pending_state_tree_diffs::block_height.asc())
            .get_results::<sql_models::PendingStateTreeDiff>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "pending_state_tree_diffs_get_all_pending",
                source: e,
            })?;

        let mut diffs = HashMap::new();
        for diff in diff_recs {
            let shard = Shard::from(diff.shard as u32);
            let diff = PendingShardStateTreeDiff::try_from(diff)?;
            diffs
                .entry(shard)
                .or_insert_with(Vec::new) //PendingStateTreeDiff::default)
                .push(diff);
        }

        Ok(diffs)
        */
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
        todo!()
        /*
        use crate::schema::state_tree;

        let node = state_tree::table
            .select(state_tree::node)
            .filter(state_tree::shard.eq(shard.as_u32() as i32))
            .filter(state_tree::key.eq(key.to_string()))
            .first::<String>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "state_tree_nodes_get",
                source: e,
            })?;

        let node = serde_json::from_str::<TreeNode<Version>>(&node).map_err(|e| StorageError::DataInconsistency {
            details: format!("Failed to deserialize state tree node: {}", e),
        })?;

        Ok(node.into_node())
        */
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
        todo!()
        /*
        use crate::schema::epoch_checkpoints;

        let checkpoint = epoch_checkpoints::table
            .filter(epoch_checkpoints::epoch.eq(epoch.as_u64() as i64))
            .first::<sql_models::EpochCheckpoint>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "epoch_checkpoint_get",
                source: e,
            })?;

        checkpoint.try_into()
        */
    }

    fn foreign_substate_pledges_exists_for_address<T: ToSubstateAddress>(
        &self,
        transaction_id: &TransactionId,
        address: T,
    ) -> Result<bool, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_substate_pledges;

        let address = address.to_substate_address();
        let count = foreign_substate_pledges::table
            .count()
            .filter(foreign_substate_pledges::transaction_id.eq(serialize_hex(transaction_id)))
            .filter(foreign_substate_pledges::address.eq(serialize_hex(address)))
            .limit(1)
            .get_result::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_substate_pledges_exists",
                source: e,
            })?;

        Ok(count > 0)
        */
    }

    fn foreign_substate_pledges_get_all_by_transaction_id(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<SubstatePledges, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_substate_pledges;

        let recs = foreign_substate_pledges::table
            .filter(foreign_substate_pledges::transaction_id.eq(serialize_hex(transaction_id)))
            .get_results::<sql_models::ForeignSubstatePledge>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_substate_pledges_get",
                source: e,
            })?;

        #[allow(clippy::mutable_key_type)]
        let mut pledges = SubstatePledges::with_capacity(recs.len());
        for pledge in recs {
            let substate_id = parse_from_string(&pledge.substate_id)?;
            let version = pledge.version as u32;
            let id = VersionedSubstateId::new(substate_id, version);
            let lock_type = parse_from_string(&pledge.lock_type)?;
            let lock_intent = VersionedSubstateIdLockIntent::new(id, lock_type, true);
            let substate_value = pledge.substate_value.as_deref().map(deserialize_json).transpose()?;
            let pledge = SubstatePledge::try_create(lock_intent.clone(), substate_value).ok_or_else(|| {
                StorageError::DataInconsistency {
                    details: format!("Invalid input substate pledge for {lock_intent}"),
                }
            })?;
            pledges.insert(pledge);
        }

        Ok(pledges)
        */
    }

    fn burnt_utxos_get(&self, commitment: &UnclaimedConfidentialOutputAddress) -> Result<BurntUtxo, StorageError> {
        todo!()
    }

    fn burnt_utxos_get_all_unproposed(
        &self,
        leaf_block: &BlockId,
        limit: usize,
    ) -> Result<Vec<BurntUtxo>, StorageError> {
        todo!()
        /*
        use crate::schema::burnt_utxos;
        if !self.blocks_exists(leaf_block)? {
            return Err(StorageError::NotFound {
                item: "Block".to_string(),
                key: leaf_block.to_string(),
            });
        }

        if limit == 0 {
            return Ok(Vec::new());
        }

        let locked_block = self.get_current_locked_block()?;
        let exclude_block_ids = self.get_block_ids_with_commands_between(&locked_block.block_id, leaf_block)?;

        let burnt_utxos = burnt_utxos::table
            .filter(
                burnt_utxos::proposed_in_block
                    .is_null()
                    .or(burnt_utxos::proposed_in_block
                        .ne_all(exclude_block_ids)
                        .and(burnt_utxos::proposed_in_block_height.gt(locked_block.height.as_u64() as i64))),
            )
            .limit(limit as i64)
            .get_results::<sql_models::BurntUtxo>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "burnt_utxos_get_all_unproposed",
                source: e,
            })?;

        burnt_utxos.into_iter().map(TryInto::try_into).collect()
        */
    }

    fn burnt_utxos_count(&self) -> Result<u64, StorageError> {
        todo!()
        /*
        use crate::schema::burnt_utxos;

        let count = burnt_utxos::table
            .count()
            .get_result::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "burnt_utxos_count",
                source: e,
            })?;

        Ok(count as u64)
        */
    }

    fn foreign_parked_blocks_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        todo!()
        /*
        use crate::schema::foreign_parked_blocks;

        let count = foreign_parked_blocks::table
            .count()
            .filter(foreign_parked_blocks::block_id.eq(serialize_hex(block_id)))
            .get_result::<i64>(self.connection())
            .map_err(|e| SqliteStorageError::DieselError {
                operation: "foreign_parked_blocks_exists",
                source: e,
            })?;

        Ok(count > 0)
        */
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
        todo!()
    }
    
    fn evicted_nodes_count(&self, epoch: Epoch) -> Result<u64, StorageError> {
        todo!()
    }
}

/*
#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub count: i64,
}
*/

/* 
#[derive(QueryableByName)]
struct BlockIdSqlValue {
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub bid: String,
}
*/
