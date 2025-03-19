//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use indexmap::IndexMap;
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight, PeerAddress, VersionedSubstateId};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockId,
        BlockTransactionExecution,
        Decision,
        Evidence,
        ForeignParkedProposal,
        ForeignProposal,
        ForeignProposalStatus,
        ForeignReceiveCounters,
        ForeignSendCounters,
        HighQc,
        LastExecuted,
        LastProposed,
        LastSentVote,
        LastVoted,
        LeafBlock,
        LockConflict,
        LockedBlock,
        NoVoteReason,
        PendingShardStateTreeDiff,
        QcId,
        QuorumCertificate,
        SubstateChange,
        SubstateLock,
        SubstatePledges,
        SubstateRecord,
        TransactionPoolRecord,
        TransactionPoolStatusUpdate,
        TransactionRecord,
        ValidatorStatsUpdate,
    },
    StateStore,
    StateStoreWriteTransaction,
    StorageError,
};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_state_tree::{Node, NodeKey, StaleTreeNode, Version};
use tari_template_lib::models::UnclaimedConfidentialOutputAddress;
use tari_transaction::TransactionId;

use crate::reader::AnyStateStoreReadTransaction;

pub enum AnyStateStoreWriteTransaction<'a> {
    Rocksdb {
        write_tx: <RocksDbStateStore<PeerAddress> as StateStore>::WriteTransaction<'a>,
        read_tx: AnyStateStoreReadTransaction<'a>,
    },
    Sqlite {
        write_tx: <SqliteStateStore<PeerAddress> as StateStore>::WriteTransaction<'a>,
        read_tx: AnyStateStoreReadTransaction<'a>,
    },
}

impl<'a> Deref for AnyStateStoreWriteTransaction<'a> {
    type Target = AnyStateStoreReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Rocksdb { read_tx, .. } => read_tx,
            Self::Sqlite { read_tx, .. } => read_tx,
        }
    }
}

impl<'tx> StateStoreWriteTransaction for AnyStateStoreWriteTransaction<'tx> {
    type Addr = PeerAddress;

    fn commit(&mut self) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.commit(),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.commit(),
        }
    }

    fn rollback(&mut self) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.rollback(),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.rollback(),
        }
    }

    fn blocks_insert(&mut self, block: &Block) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.blocks_insert(block),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.blocks_insert(block),
        }
    }

    fn blocks_delete(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.blocks_delete(block_id),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.blocks_delete(block_id),
        }
    }

    fn blocks_set_flags(
        &mut self,
        block_id: &BlockId,
        is_committed: Option<bool>,
        is_justified: Option<bool>,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.blocks_set_flags(block_id, is_committed, is_justified)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.blocks_set_flags(block_id, is_committed, is_justified)
            },
        }
    }

    fn block_diffs_insert(&mut self, block_id: &BlockId, changes: &[SubstateChange]) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.block_diffs_insert(block_id, changes),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.block_diffs_insert(block_id, changes),
        }
    }

    fn block_diffs_remove(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.block_diffs_remove(block_id),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.block_diffs_remove(block_id),
        }
    }

    fn quorum_certificates_insert(&mut self, qc: &QuorumCertificate) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.quorum_certificates_insert(qc),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.quorum_certificates_insert(qc),
        }
    }

    fn quorum_certificates_set_shares_processed(&mut self, qc_id: &QcId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.quorum_certificates_set_shares_processed(qc_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.quorum_certificates_set_shares_processed(qc_id)
            },
        }
    }

    fn last_sent_vote_set(&mut self, last_sent_vote: &LastSentVote) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_sent_vote_set(last_sent_vote),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.last_sent_vote_set(last_sent_vote),
        }
    }

    fn last_voted_set(&mut self, last_voted: &LastVoted) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_voted_set(last_voted),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.last_voted_set(last_voted),
        }
    }

    fn last_executed_set(&mut self, last_exec: &LastExecuted) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_executed_set(last_exec),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.last_executed_set(last_exec),
        }
    }

    fn last_proposed_set(&mut self, last_proposed: &LastProposed) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_proposed_set(last_proposed),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.last_proposed_set(last_proposed),
        }
    }

    fn leaf_block_set(&mut self, leaf_node: &LeafBlock) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.leaf_block_set(leaf_node),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.leaf_block_set(leaf_node),
        }
    }

    fn locked_block_set(&mut self, locked_block: &LockedBlock) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.locked_block_set(locked_block),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.locked_block_set(locked_block),
        }
    }

    fn high_qc_set(&mut self, high_qc: &HighQc) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.high_qc_set(high_qc),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.high_qc_set(high_qc),
        }
    }

    fn foreign_proposals_delete(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_delete(block_id),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.foreign_proposals_delete(block_id),
        }
    }

    fn foreign_proposals_delete_in_epoch(&mut self, epoch: Epoch) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_proposals_delete_in_epoch(epoch)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.foreign_proposals_delete_in_epoch(epoch),
        }
    }

    fn foreign_proposals_set_status(
        &mut self,
        block_id: &BlockId,
        status: ForeignProposalStatus,
        set_proposed_in_block: Option<&LeafBlock>,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_proposals_set_status(block_id, status, set_proposed_in_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_proposals_set_status(block_id, status, set_proposed_in_block)
            },
        }
    }

    fn foreign_proposals_clear_proposed_in(&mut self, proposed_in_block: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_proposals_clear_proposed_in(proposed_in_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_proposals_clear_proposed_in(proposed_in_block)
            },
        }
    }

    fn foreign_send_counters_set(
        &mut self,
        foreign_send_counter: &ForeignSendCounters,
        block_id: &BlockId,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_send_counters_set(foreign_send_counter, block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_send_counters_set(foreign_send_counter, block_id)
            },
        }
    }

    fn foreign_receive_counters_set(
        &mut self,
        foreign_send_counter: &ForeignReceiveCounters,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_receive_counters_set(foreign_send_counter)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_receive_counters_set(foreign_send_counter)
            },
        }
    }

    fn transactions_insert(&mut self, transaction: &TransactionRecord) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transactions_insert(transaction),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.transactions_insert(transaction),
        }
    }

    fn transactions_finalize_all<'a, I: IntoIterator<Item = &'a TransactionPoolRecord>>(
        &mut self,
        block_id: BlockId,
        transaction: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transactions_finalize_all(block_id, transaction)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transactions_finalize_all(block_id, transaction)
            },
        }
    }

    fn transaction_executions_insert_or_ignore(
        &mut self,
        transaction_execution: &BlockTransactionExecution,
    ) -> Result<bool, StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_executions_insert_or_ignore(transaction_execution)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_executions_insert_or_ignore(transaction_execution)
            },
        }
    }

    fn transaction_executions_remove_any_by_block_id(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_executions_remove_any_by_block_id(block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_executions_remove_any_by_block_id(block_id)
            },
        }
    }

    fn transaction_pool_insert_new(
        &mut self,
        tx_id: TransactionId,
        decision: Decision,
        initial_evidence: &Evidence,
        is_ready: bool,
        is_global: bool,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_pool_insert_new(tx_id, decision, initial_evidence, is_ready, is_global)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_pool_insert_new(tx_id, decision, initial_evidence, is_ready, is_global)
            },
        }
    }

    fn transaction_pool_add_pending_update(
        &mut self,
        block_id: &BlockId,
        pool_update: &TransactionPoolStatusUpdate,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_pool_add_pending_update(block_id, pool_update)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_pool_add_pending_update(block_id, pool_update)
            },
        }
    }

    fn transaction_pool_remove_all<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<Vec<TransactionPoolRecord>, StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_pool_remove_all(transaction_ids)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_pool_remove_all(transaction_ids)
            },
        }
    }

    fn transaction_pool_confirm_all_transitions(&mut self, block: &LeafBlock) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_pool_confirm_all_transitions(block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_pool_confirm_all_transitions(block)
            },
        }
    }

    fn transaction_pool_state_updates_remove_any_by_block_id(
        &mut self,
        block_id: &BlockId,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.transaction_pool_state_updates_remove_any_by_block_id(block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.transaction_pool_state_updates_remove_any_by_block_id(block_id)
            },
        }
    }

    fn missing_transactions_insert<'a, IMissing: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        park_block: &Block,
        foreign_proposals: &[ForeignProposal],
        missing_transaction_ids: IMissing,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.missing_transactions_insert(park_block, foreign_proposals, missing_transaction_ids)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.missing_transactions_insert(park_block, foreign_proposals, missing_transaction_ids)
            },
        }
    }

    fn missing_transactions_remove(
        &mut self,
        height: NodeHeight,
        transaction_id: &TransactionId,
    ) -> Result<Option<(Block, Vec<ForeignProposal>)>, StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.missing_transactions_remove(height, transaction_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.missing_transactions_remove(height, transaction_id)
            },
        }
    }

    fn foreign_parked_blocks_insert(&mut self, park_block: &ForeignParkedProposal) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_parked_blocks_insert(park_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.foreign_parked_blocks_insert(park_block),
        }
    }

    fn foreign_parked_blocks_insert_missing_transactions<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        park_block_id: &BlockId,
        missing_transaction_ids: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_parked_blocks_insert_missing_transactions(park_block_id, missing_transaction_ids)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_parked_blocks_insert_missing_transactions(park_block_id, missing_transaction_ids)
            },
        }
    }

    fn foreign_parked_blocks_remove_all_by_transaction(
        &mut self,
        transaction_id: &TransactionId,
    ) -> Result<Vec<ForeignParkedProposal>, StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_parked_blocks_remove_all_by_transaction(transaction_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_parked_blocks_remove_all_by_transaction(transaction_id)
            },
        }
    }

    fn votes_insert(&mut self, vote: &Vote) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.votes_insert(vote),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.votes_insert(vote),
        }
    }

    fn votes_delete_all(&mut self) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.votes_delete_all(),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.votes_delete_all(),
        }
    }

    fn substate_locks_insert_all<
        'a,
        I: IntoIterator<Item = (&'a tari_engine_types::substate::SubstateId, &'a Vec<SubstateLock>)>,
    >(
        &mut self,
        block_id: &BlockId,
        locks: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.substate_locks_insert_all(block_id, locks)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.substate_locks_insert_all(block_id, locks)
            },
        }
    }

    fn substate_locks_remove_many_for_transactions<'a, I: Iterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.substate_locks_remove_many_for_transactions(transaction_ids)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.substate_locks_remove_many_for_transactions(transaction_ids)
            },
        }
    }

    fn substate_locks_remove_any_by_block_id(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.substate_locks_remove_any_by_block_id(block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.substate_locks_remove_any_by_block_id(block_id)
            },
        }
    }

    fn substates_create(&mut self, substate: &SubstateRecord) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substates_create(substate),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.substates_create(substate),
        }
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
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substates_down(
                versioned_substate_id,
                shard,
                epoch,
                destroyed_block_height,
                destroyed_transaction_id,
                destroyed_qc_id,
            ),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.substates_down(
                versioned_substate_id,
                shard,
                epoch,
                destroyed_block_height,
                destroyed_transaction_id,
                destroyed_qc_id,
            ),
        }
    }

    fn foreign_substate_pledges_save(
        &mut self,
        transaction_id: &TransactionId,
        shard_group: ShardGroup,
        pledges: &SubstatePledges,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_substate_pledges_save(transaction_id, shard_group, pledges)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_substate_pledges_save(transaction_id, shard_group, pledges)
            },
        }
    }

    fn foreign_substate_pledges_remove_many<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_substate_pledges_remove_many(transaction_ids)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.foreign_substate_pledges_remove_many(transaction_ids)
            },
        }
    }

    fn pending_state_tree_diffs_insert(
        &mut self,
        block_id: BlockId,
        shard: Shard,
        diff: &PendingShardStateTreeDiff,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.pending_state_tree_diffs_insert(block_id, shard, diff)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.pending_state_tree_diffs_insert(block_id, shard, diff)
            },
        }
    }

    fn pending_state_tree_diffs_remove_by_block(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.pending_state_tree_diffs_remove_by_block(block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.pending_state_tree_diffs_remove_by_block(block_id)
            },
        }
    }

    fn pending_state_tree_diffs_remove_and_return_by_block(
        &mut self,
        block_id: &BlockId,
    ) -> Result<IndexMap<Shard, Vec<PendingShardStateTreeDiff>>, StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.pending_state_tree_diffs_remove_and_return_by_block(block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.pending_state_tree_diffs_remove_and_return_by_block(block_id)
            },
        }
    }

    fn state_tree_nodes_insert(&mut self, shard: Shard, key: NodeKey, node: Node<Version>) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.state_tree_nodes_insert(shard, key, node)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.state_tree_nodes_insert(shard, key, node)
            },
        }
    }

    fn state_tree_nodes_record_stale_tree_node(
        &mut self,
        shard: Shard,
        node: StaleTreeNode,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.state_tree_nodes_record_stale_tree_node(shard, node)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.state_tree_nodes_record_stale_tree_node(shard, node)
            },
        }
    }

    fn state_tree_shard_versions_set(&mut self, shard: Shard, version: Version) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.state_tree_shard_versions_set(shard, version)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.state_tree_shard_versions_set(shard, version)
            },
        }
    }

    fn epoch_checkpoint_save(&mut self, checkpoint: &EpochCheckpoint) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.epoch_checkpoint_save(checkpoint),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.epoch_checkpoint_save(checkpoint),
        }
    }

    fn burnt_utxos_insert(&mut self, burnt_utxo: &BurntUtxo) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.burnt_utxos_insert(burnt_utxo),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.burnt_utxos_insert(burnt_utxo),
        }
    }

    fn burnt_utxos_set_proposed_block(
        &mut self,
        commitment: &UnclaimedConfidentialOutputAddress,
        proposed_in_block: &BlockId,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.burnt_utxos_set_proposed_block(commitment, proposed_in_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.burnt_utxos_set_proposed_block(commitment, proposed_in_block)
            },
        }
    }

    fn burnt_utxos_clear_proposed_block(&mut self, proposed_in_block: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.burnt_utxos_clear_proposed_block(proposed_in_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.burnt_utxos_clear_proposed_block(proposed_in_block)
            },
        }
    }

    fn burnt_utxos_delete(
        &mut self,
        commitment: &UnclaimedConfidentialOutputAddress,
        proposed_in_block: &BlockId,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.burnt_utxos_delete(commitment, proposed_in_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.burnt_utxos_delete(commitment, proposed_in_block)
            },
        }
    }

    fn lock_conflicts_insert_all<'a, I: IntoIterator<Item = (&'a TransactionId, &'a Vec<LockConflict>)>>(
        &mut self,
        block_id: &BlockId,
        conflicts: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.lock_conflicts_insert_all(block_id, conflicts)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.lock_conflicts_insert_all(block_id, conflicts)
            },
        }
    }

    fn validator_epoch_stats_updates<'a, I: IntoIterator<Item = ValidatorStatsUpdate<'a>>>(
        &mut self,
        epoch: Epoch,
        updates: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.validator_epoch_stats_updates(epoch, updates)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.validator_epoch_stats_updates(epoch, updates)
            },
        }
    }

    fn diagnostics_add_no_vote(&mut self, block_id: BlockId, reason: NoVoteReason) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.diagnostics_add_no_vote(block_id, reason)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.diagnostics_add_no_vote(block_id, reason)
            },
        }
    }

    fn lock_conflicts_remove_by_transaction_ids<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.lock_conflicts_remove_by_transaction_ids(transaction_ids)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.lock_conflicts_remove_by_transaction_ids(transaction_ids)
            },
        }
    }

    fn lock_conflicts_remove_by_block_id(&mut self, block_id: &BlockId) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.lock_conflicts_remove_by_block_id(block_id)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.lock_conflicts_remove_by_block_id(block_id)
            },
        }
    }

    fn evicted_nodes_evict(
        &mut self,
        public_key: &tari_common_types::types::PublicKey,
        evicted_in_block: BlockId,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.evicted_nodes_evict(public_key, evicted_in_block)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.evicted_nodes_evict(public_key, evicted_in_block)
            },
        }
    }

    fn evicted_nodes_mark_eviction_as_committed(
        &mut self,
        public_key: &tari_common_types::types::PublicKey,
        epoch: Epoch,
    ) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.evicted_nodes_mark_eviction_as_committed(public_key, epoch)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => {
                write_tx.evicted_nodes_mark_eviction_as_committed(public_key, epoch)
            },
        }
    }

    fn foreign_proposals_save(&mut self, foreign_proposal: &ForeignProposal) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => {
                write_tx.foreign_proposals_save(foreign_proposal)
            },
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.foreign_proposals_save(foreign_proposal),
        }
    }

    fn substates_prune_downed_values(&mut self, epoch: Epoch) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substates_prune_downed_values(epoch),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.substates_prune_downed_values(epoch),
        }
    }

    fn state_tree_nodes_clear_stale(&mut self, limit: usize) -> Result<(), StorageError> {
        match self {
            AnyStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.state_tree_nodes_clear_stale(limit),
            AnyStateStoreWriteTransaction::Sqlite { write_tx, .. } => write_tx.state_tree_nodes_clear_stale(limit),
        }
    }
}
