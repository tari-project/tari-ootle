//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, HashSet};

use tari_common_types::types::{FixedHash, PublicKey};
use tari_dan_common_types::{
    shard::Shard,
    Epoch,
    NodeHeight,
    PeerAddress,
    SubstateAddress,
    ToSubstateAddress,
    VersionedSubstateId,
    VersionedSubstateIdRef,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockDiff,
        BlockId,
        BlockTransactionExecution,
        BurntUtxo,
        EpochCheckpoint,
        ForeignProposal,
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
        SubstatePledges,
        SubstateRecord,
        TransactionPoolConfirmedStage,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
        ValidatorConsensusStats,
        Vote,
    },
    StateStore,
    StateStoreReadTransaction,
    StorageError,
};
use tari_engine_types::substate::SubstateId;
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_state_tree::{Node, NodeKey, Version};
use tari_template_lib::models::UnclaimedConfidentialOutputAddress;
use tari_transaction::TransactionId;

pub enum AnyStateStoreReadTransaction<'a> {
    Rocksdb(<RocksDbStateStore<PeerAddress> as StateStore>::ReadTransaction<'a>),
    RocksdbRef(&'a <RocksDbStateStore<PeerAddress> as StateStore>::ReadTransaction<'a>),
    Sqlite(<SqliteStateStore<PeerAddress> as StateStore>::ReadTransaction<'a>),
    SqliteRef(&'a <SqliteStateStore<PeerAddress> as StateStore>::ReadTransaction<'a>),
}

impl<'tx> StateStoreReadTransaction for AnyStateStoreReadTransaction<'tx> {
    type Addr = PeerAddress;

    fn last_sent_vote_get(&self) -> Result<LastSentVote, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.last_sent_vote_get(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.last_sent_vote_get(),
        }
    }

    fn last_voted_get(&self) -> Result<LastVoted, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.last_voted_get(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.last_voted_get(),
        }
    }

    fn last_executed_get(&self) -> Result<LastExecuted, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.last_executed_get(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.last_executed_get(),
        }
    }

    fn last_proposed_get(&self) -> Result<LastProposed, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.last_proposed_get(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.last_proposed_get(),
        }
    }

    fn locked_block_get(&self, epoch: Epoch) -> Result<LockedBlock, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.locked_block_get(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.locked_block_get(epoch),
        }
    }

    fn leaf_block_get(&self, epoch: Epoch) -> Result<LeafBlock, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.leaf_block_get(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.leaf_block_get(epoch),
        }
    }

    fn high_qc_get(&self, epoch: Epoch) -> Result<HighQc, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.high_qc_get(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.high_qc_get(epoch),
        }
    }

    fn foreign_proposals_get_any<'a, I: IntoIterator<Item = &'a BlockId>>(
        &self,
        block_ids: I,
    ) -> Result<Vec<ForeignProposal>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_proposals_get_any(block_ids),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_proposals_get_any(block_ids),
        }
    }

    fn foreign_proposals_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_proposals_exists(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_proposals_exists(block_id),
        }
    }

    fn foreign_proposals_has_unconfirmed(&self, epoch: Epoch) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_proposals_has_unconfirmed(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_proposals_has_unconfirmed(epoch),
        }
    }

    fn foreign_proposals_get_all_new(
        &self,
        block_id: &BlockId,
        limit: usize,
    ) -> Result<Vec<ForeignProposal>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_proposals_get_all_new(block_id, limit),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_proposals_get_all_new(block_id, limit),
        }
    }

    fn foreign_send_counters_get(&self, block_id: &BlockId) -> Result<ForeignSendCounters, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_send_counters_get(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_send_counters_get(block_id),
        }
    }

    fn foreign_receive_counters_get(&self) -> Result<ForeignReceiveCounters, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_receive_counters_get(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_receive_counters_get(),
        }
    }

    fn transactions_get(&self, tx_id: &TransactionId) -> Result<TransactionRecord, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transactions_get(tx_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transactions_get(tx_id),
        }
    }

    fn transactions_exists(&self, tx_id: &TransactionId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transactions_exists(tx_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transactions_exists(tx_id),
        }
    }

    fn transactions_get_any<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &self,
        tx_ids: I,
    ) -> Result<Vec<TransactionRecord>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transactions_get_any(tx_ids),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transactions_get_any(tx_ids),
        }
    }

    fn transaction_executions_get(
        &self,
        tx_id: &TransactionId,
        block: &BlockId,
    ) -> Result<BlockTransactionExecution, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transaction_executions_get(tx_id, block),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_executions_get(tx_id, block),
        }
    }

    fn transaction_executions_get_pending_for_block(
        &self,
        tx_id: &TransactionId,
        from_block_id: &BlockId,
    ) -> Result<BlockTransactionExecution, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.transaction_executions_get_pending_for_block(tx_id, from_block_id)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.transaction_executions_get_pending_for_block(tx_id, from_block_id)
            },
        }
    }

    fn blocks_get(&self, block_id: &BlockId) -> Result<Block, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_get(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_get(block_id),
        }
    }

    fn blocks_get_all_ids_by_height(&self, epoch: Epoch, height: NodeHeight) -> Result<Vec<BlockId>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_get_all_ids_by_height(epoch, height),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_get_all_ids_by_height(epoch, height),
        }
    }

    fn blocks_get_genesis_for_epoch(&self, epoch: Epoch) -> Result<Block, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_get_genesis_for_epoch(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_get_genesis_for_epoch(epoch),
        }
    }

    fn blocks_get_last_n_in_epoch(&self, n: usize, epoch: Epoch) -> Result<Vec<Block>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_get_last_n_in_epoch(n, epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_get_last_n_in_epoch(n, epoch),
        }
    }

    fn blocks_get_all_between(
        &self,
        epoch: Epoch,
        start_block_height: NodeHeight,
        end_block_height: NodeHeight,
        include_dummy_blocks: bool,
        limit: usize,
    ) -> Result<Vec<Block>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.blocks_get_all_between(epoch, start_block_height, end_block_height, include_dummy_blocks, limit)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.blocks_get_all_between(epoch, start_block_height, end_block_height, include_dummy_blocks, limit)
            },
        }
    }

    fn blocks_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_exists(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_exists(block_id),
        }
    }

    fn blocks_is_ancestor(&self, descendant: &BlockId, ancestor: &BlockId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_is_ancestor(descendant, ancestor),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_is_ancestor(descendant, ancestor),
        }
    }

    fn blocks_get_paginated(
        &self,
        limit: u64,
        offset: u64,
        filter_index: Option<usize>,
        filter: Option<String>,
        ordering_index: Option<usize>,
        ordering: Option<tari_dan_storage::Ordering>,
    ) -> Result<Vec<Block>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.blocks_get_paginated(limit, offset, filter_index, filter, ordering_index, ordering)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.blocks_get_paginated(limit, offset, filter_index, filter, ordering_index, ordering)
            },
        }
    }

    fn filtered_blocks_get_count(
        &self,
        filter_index: Option<usize>,
        filter: Option<String>,
    ) -> Result<i64, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.filtered_blocks_get_count(filter_index, filter),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.filtered_blocks_get_count(filter_index, filter),
        }
    }

    fn block_diffs_get(&self, block_id: &BlockId) -> Result<BlockDiff, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.block_diffs_get(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.block_diffs_get(block_id),
        }
    }

    fn block_diffs_get_last_change_for_substate(
        &self,
        block_id: &BlockId,
        substate_id: &tari_engine_types::substate::SubstateId,
    ) -> Result<SubstateChange, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.block_diffs_get_last_change_for_substate(block_id, substate_id)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.block_diffs_get_last_change_for_substate(block_id, substate_id)
            },
        }
    }

    fn quorum_certificates_get(&self, qc_id: &QcId) -> Result<QuorumCertificate, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.quorum_certificates_get(qc_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.quorum_certificates_get(qc_id),
        }
    }

    fn quorum_certificates_get_all<'a, I>(&self, qc_ids: I) -> Result<Vec<QuorumCertificate>, StorageError>
    where
        I: IntoIterator<Item = &'a QcId>,
        I::IntoIter: ExactSizeIterator,
    {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.quorum_certificates_get_all(qc_ids),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.quorum_certificates_get_all(qc_ids),
        }
    }

    fn quorum_certificates_get_by_block_id(&self, block_id: &BlockId) -> Result<QuorumCertificate, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.quorum_certificates_get_by_block_id(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.quorum_certificates_get_by_block_id(block_id),
        }
    }

    fn transaction_pool_get_for_blocks(
        &self,
        to_block_id: &BlockId,
        transaction_id: &TransactionId,
    ) -> Result<TransactionPoolRecord, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transaction_pool_get_for_blocks(to_block_id, transaction_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_pool_get_for_blocks(to_block_id, transaction_id),
        }
    }

    fn transaction_pool_exists(&self, transaction_id: &TransactionId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transaction_pool_exists(transaction_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_pool_exists(transaction_id),
        }
    }

    fn transaction_pool_get_all(&self) -> Result<Vec<TransactionPoolRecord>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transaction_pool_get_all(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_pool_get_all(),
        }
    }

    fn transaction_pool_get_many_ready(
        &self,
        max_txs: usize,
        block_id: &BlockId,
    ) -> Result<Vec<TransactionPoolRecord>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transaction_pool_get_many_ready(max_txs, block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_pool_get_many_ready(max_txs, block_id),
        }
    }

    fn transaction_pool_count(
        &self,
        stage: Option<TransactionPoolStage>,
        is_ready: Option<bool>,
        skip_lock_conflicted: bool,
    ) -> Result<usize, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.transaction_pool_count(stage, is_ready, skip_lock_conflicted)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_pool_count(stage, is_ready, skip_lock_conflicted),
        }
    }

    fn votes_get_by_block_and_sender(
        &self,
        block_id: &BlockId,
        sender_leaf_hash: &FixedHash,
    ) -> Result<Vote, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.votes_get_by_block_and_sender(block_id, sender_leaf_hash),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.votes_get_by_block_and_sender(block_id, sender_leaf_hash),
        }
    }

    fn votes_count_for_block(&self, block_id: &BlockId) -> Result<u64, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.votes_count_for_block(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.votes_count_for_block(block_id),
        }
    }

    fn votes_get_for_block(&self, block_id: &BlockId) -> Result<Vec<Vote>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.votes_get_for_block(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.votes_get_for_block(block_id),
        }
    }

    fn substates_get(&self, address: &SubstateAddress) -> Result<SubstateRecord, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_get(address),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_get(address),
        }
    }

    fn substates_get_any_max_version<'a, I>(&self, substate_ids: I) -> Result<Vec<SubstateRecord>, StorageError>
    where
        I: IntoIterator<Item = &'a SubstateId>,
        I::IntoIter: ExactSizeIterator,
    {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_get_any_max_version(substate_ids),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_get_any_max_version(substate_ids),
        }
    }

    fn substates_get_max_version_for_substate(&self, substate_id: &SubstateId) -> Result<(u32, bool), StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_get_max_version_for_substate(substate_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_get_max_version_for_substate(substate_id),
        }
    }

    fn substates_any_exist<I, S>(&self, substates: I) -> Result<bool, StorageError>
    where
        I: IntoIterator<Item = S>,
        S: std::borrow::Borrow<VersionedSubstateId>,
    {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_any_exist(substates),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_any_exist(substates),
        }
    }

    fn substates_exists_for_transaction(&self, transaction_id: &TransactionId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_exists_for_transaction(transaction_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_exists_for_transaction(transaction_id),
        }
    }

    fn substates_get_all_for_transaction(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_get_all_for_transaction(transaction_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_get_all_for_transaction(transaction_id),
        }
    }

    fn substate_locks_get_locked_substates_for_transaction(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<Vec<LockedSubstateValue>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.substate_locks_get_locked_substates_for_transaction(transaction_id)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.substate_locks_get_locked_substates_for_transaction(transaction_id)
            },
        }
    }

    fn substate_locks_get_latest_for_substate(&self, substate_id: &SubstateId) -> Result<SubstateLock, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substate_locks_get_latest_for_substate(substate_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substate_locks_get_latest_for_substate(substate_id),
        }
    }

    fn pending_state_tree_diffs_get_all_up_to_commit_block(
        &self,
        block_id: &BlockId,
    ) -> Result<HashMap<Shard, Vec<PendingShardStateTreeDiff>>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_id)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_id),
        }
    }

    fn state_transitions_get_n_after(
        &self,
        n: usize,
        id: StateTransitionId,
        end_epoch: Epoch,
    ) -> Result<Vec<StateTransition>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.state_transitions_get_n_after(n, id, end_epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.state_transitions_get_n_after(n, id, end_epoch),
        }
    }

    fn state_transitions_get_last_id(&self, shard: Shard) -> Result<StateTransitionId, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.state_transitions_get_last_id(shard),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.state_transitions_get_last_id(shard),
        }
    }

    fn state_tree_nodes_get(&self, shard: Shard, key: &NodeKey) -> Result<Node<Version>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.state_tree_nodes_get(shard, key),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.state_tree_nodes_get(shard, key),
        }
    }

    fn state_tree_versions_get_latest(&self, shard: Shard) -> Result<Option<Version>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.state_tree_versions_get_latest(shard),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.state_tree_versions_get_latest(shard),
        }
    }

    fn epoch_checkpoint_get(&self, epoch: Epoch) -> Result<EpochCheckpoint, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.epoch_checkpoint_get(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.epoch_checkpoint_get(epoch),
        }
    }

    fn foreign_substate_pledges_get_all_by_transaction_id(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<SubstatePledges, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.foreign_substate_pledges_get_all_by_transaction_id(transaction_id)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.foreign_substate_pledges_get_all_by_transaction_id(transaction_id)
            },
        }
    }

    fn burnt_utxos_get(&self, commitment: &UnclaimedConfidentialOutputAddress) -> Result<BurntUtxo, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.burnt_utxos_get(commitment),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.burnt_utxos_get(commitment),
        }
    }

    fn burnt_utxos_get_all_unproposed(
        &self,
        leaf_block: &BlockId,
        limit: usize,
    ) -> Result<Vec<BurntUtxo>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.burnt_utxos_get_all_unproposed(leaf_block, limit),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.burnt_utxos_get_all_unproposed(leaf_block, limit),
        }
    }

    fn burnt_utxos_count(&self) -> Result<u64, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.burnt_utxos_count(),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.burnt_utxos_count(),
        }
    }

    fn foreign_parked_blocks_exists(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.foreign_parked_blocks_exists(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.foreign_parked_blocks_exists(block_id),
        }
    }

    fn validator_epoch_stats_get(
        &self,
        epoch: Epoch,
        public_key: &PublicKey,
    ) -> Result<ValidatorConsensusStats, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.validator_epoch_stats_get(epoch, public_key),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.validator_epoch_stats_get(epoch, public_key),
        }
    }

    fn validator_epoch_stats_get_nodes_to_evict(
        &self,
        block_id: &BlockId,
        threshold: u64,
        limit: u64,
    ) -> Result<Vec<PublicKey>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.validator_epoch_stats_get_nodes_to_evict(block_id, threshold, limit)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.validator_epoch_stats_get_nodes_to_evict(block_id, threshold, limit)
            },
        }
    }

    fn suspended_nodes_is_evicted(&self, block_id: &BlockId, public_key: &PublicKey) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.suspended_nodes_is_evicted(block_id, public_key),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.suspended_nodes_is_evicted(block_id, public_key),
        }
    }

    fn evicted_nodes_count(&self, epoch: Epoch) -> Result<u64, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.evicted_nodes_count(epoch),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.evicted_nodes_count(epoch),
        }
    }

    fn transaction_pool_has_pending_state_updates(&self, block_id: &BlockId) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.transaction_pool_has_pending_state_updates(block_id),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.transaction_pool_has_pending_state_updates(block_id),
        }
    }

    fn block_diffs_get_change_for_versioned_substate<'a, T: Into<VersionedSubstateIdRef<'a>>>(
        &self,
        block_id: &BlockId,
        substate_id: T,
    ) -> Result<SubstateChange, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.block_diffs_get_change_for_versioned_substate(block_id, substate_id)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.block_diffs_get_change_for_versioned_substate(block_id, substate_id)
            },
        }
    }

    fn substate_locks_has_any_write_locks_for_substates<'a, I: IntoIterator<Item = &'a SubstateId>>(
        &self,
        exclude_transaction_id: Option<&TransactionId>,
        substate_ids: I,
    ) -> Result<Option<TransactionId>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.substate_locks_has_any_write_locks_for_substates(exclude_transaction_id, substate_ids)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.substate_locks_has_any_write_locks_for_substates(exclude_transaction_id, substate_ids)
            },
        }
    }

    fn foreign_substate_pledges_exists_for_transaction_and_address<T: ToSubstateAddress>(
        &self,
        transaction_id: &TransactionId,
        address: T,
    ) -> Result<bool, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.foreign_substate_pledges_exists_for_transaction_and_address(transaction_id, address)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.foreign_substate_pledges_exists_for_transaction_and_address(transaction_id, address)
            },
        }
    }

    fn foreign_substate_pledges_get_write_pledges_to_transaction<'a, I: IntoIterator<Item = &'a SubstateId>>(
        &self,
        transaction_id: &TransactionId,
        substate_ids: I,
    ) -> Result<SubstatePledges, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => {
                tx.foreign_substate_pledges_get_write_pledges_to_transaction(transaction_id, substate_ids)
            },
            Self::Sqlite(tx) | Self::SqliteRef(tx) => {
                tx.foreign_substate_pledges_get_write_pledges_to_transaction(transaction_id, substate_ids)
            },
        }
    }

    fn substates_get_any<'a, I: IntoIterator<Item = &'a VersionedSubstateIdRef<'a>>>(
        &self,
        substate_ids: I,
    ) -> Result<Vec<SubstateRecord>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.substates_get_any(substate_ids),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.substates_get_any(substate_ids),
        }
    }

    fn blocks_get_committed_by_parent(&self, parent: &BlockId) -> Result<Block, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_get_committed_by_parent(parent),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_get_committed_by_parent(parent),
        }
    }

    fn blocks_get_pending_ids_by_parent(&self, parent: &BlockId) -> Result<Vec<BlockId>, StorageError> {
        match self {
            Self::Rocksdb(tx) | Self::RocksdbRef(tx) => tx.blocks_get_pending_ids_by_parent(parent),
            Self::Sqlite(tx) | Self::SqliteRef(tx) => tx.blocks_get_pending_ids_by_parent(parent),
        }
    }
}
