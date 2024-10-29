//   Copyright 2024. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::ops::Deref;

use anyhow::Context;
use tari_dan_common_types::{NodeAddressable, PeerAddress};
use tari_dan_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use serde::{de::DeserializeOwned, Serialize};
use tari_state_tree::{Node, NodeKey, StaleTreeNode, Version};

use crate::{config::DatabaseType, ApplicationConfig};

const LOG_TARGET: &str = "tari::dan::validator_node::state_store";

// FIXME: this is just a workaround to be able to select between Sqlite and Rocksdb implementations
// but instead we should refactor the State store trait so we can have "dyn StateStore" values
#[derive(Debug, Clone)]
pub enum ValidatorNodeStateStore {
    Rocksdb(RocksDbStateStore<PeerAddress>),
    Sqlite(SqliteStateStore<PeerAddress>),
}

impl ValidatorNodeStateStore {
    pub fn connect(config: &ApplicationConfig) -> Result<Self, anyhow::Error> {
        let state_store  = match config.validator_node.database_type {
            DatabaseType::Rocksdb => {
                let state_db_path = config.validator_node.state_db_path();
                let rocksdb_path = state_db_path.as_path()
                                    .join("rocksdb").as_os_str()
                                    .to_str()
                                    .context("committee size must be non-zero")?
                                    .to_string();
                let db: RocksDbStateStore<PeerAddress> = RocksDbStateStore::connect(&rocksdb_path)?;
                Self::Rocksdb(db)
            },
            DatabaseType::Sqlite => {
                let sqlite_connection_str = format!("sqlite://{}", config.validator_node.state_db_path().display());
                let db: SqliteStateStore<PeerAddress> = SqliteStateStore::connect(&sqlite_connection_str)?;
                Self::Sqlite(db)
            } 
        };
    
        Ok(state_store)
    }
}

impl StateStore for ValidatorNodeStateStore {
    type Addr = PeerAddress;
    
    type ReadTransaction<'a> = ValidatorNodeStateStoreReadTransaction<'a>;
    
    type WriteTransaction<'a> = ValidatorNodeStateStoreWriteTransaction<'a>;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStore::Rocksdb(db) => {
                let tx = db.create_read_tx()?;
                Ok(ValidatorNodeStateStoreReadTransaction::Rocksdb(tx))
            },
            ValidatorNodeStateStore::Sqlite(db) => {
                let tx = db.create_read_tx()?;
                Ok(ValidatorNodeStateStoreReadTransaction::Sqlite(tx))
            },
        }
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStore::Rocksdb(db) => {
                let tx = db.create_read_tx()?;
                let tx = ValidatorNodeStateStoreReadTransaction::Rocksdb(tx);
                Ok(ValidatorNodeStateStoreWriteTransaction {tx: Some(tx) } )
            },
            ValidatorNodeStateStore::Sqlite(db) => {
                let tx = db.create_read_tx()?;
                let tx = ValidatorNodeStateStoreReadTransaction::Sqlite(tx);
                Ok(ValidatorNodeStateStoreWriteTransaction {tx: Some(tx) } )
            },
        }
    }
    
    fn with_write_tx<F: FnOnce(&mut Self::WriteTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<tari_dan_storage::StorageError> {
        let mut tx = self.create_write_tx()?;
        match f(&mut tx) {
            Ok(r) => {
                tx.commit()?;
                Ok(r)
            },
            Err(e) => {
                if let Err(err) = tx.rollback() {
                    log::error!(target: LOG_TARGET, "Failed to rollback transaction: {}", err);
                }
                Err(e)
            },
        }
    }
    
    fn with_read_tx<F: FnOnce(&Self::ReadTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<tari_dan_storage::StorageError> {
        let tx = self.create_read_tx()?;
        let ret = f(&tx)?;
        Ok(ret)
    }
}

pub struct ValidatorNodeStateStoreWriteTransaction<'a> {
    tx: Option<ValidatorNodeStateStoreReadTransaction<'a>>
}

impl<'a> Deref for ValidatorNodeStateStoreWriteTransaction<'a> {
    type Target = ValidatorNodeStateStoreReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        self.tx.as_ref().unwrap()
    }
}


impl<'tx> StateStoreWriteTransaction
    for ValidatorNodeStateStoreWriteTransaction<'tx>
{
    type Addr = PeerAddress;
    
    fn commit(&mut self) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn rollback(&mut self) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_insert(&mut self, block: &tari_dan_storage::consensus_models::Block) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_delete(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_set_flags(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        is_committed: Option<bool>,
        is_justified: Option<bool>,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn block_diffs_insert(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId, changes: &[tari_dan_storage::consensus_models::SubstateChange]) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn block_diffs_remove(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn quorum_certificates_insert(&mut self, qc: &tari_dan_storage::consensus_models::QuorumCertificate) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn quorum_certificates_set_shares_processed(&mut self, qc_id: &tari_dan_storage::consensus_models::QcId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_sent_vote_set(&mut self, last_sent_vote: &tari_dan_storage::consensus_models::LastSentVote) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_voted_set(&mut self, last_voted: &tari_dan_storage::consensus_models::LastVoted) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_votes_unset(&mut self, last_voted: &tari_dan_storage::consensus_models::LastVoted) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_executed_set(&mut self, last_exec: &tari_dan_storage::consensus_models::LastExecuted) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_proposed_set(&mut self, last_proposed: &tari_dan_storage::consensus_models::LastProposed) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_proposed_unset(&mut self, last_proposed: &tari_dan_storage::consensus_models::LastProposed) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn leaf_block_set(&mut self, leaf_node: &tari_dan_storage::consensus_models::LeafBlock) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn locked_block_set(&mut self, locked_block: &tari_dan_storage::consensus_models::LockedBlock) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn high_qc_set(&mut self, high_qc: &tari_dan_storage::consensus_models::HighQc) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_upsert(
        &mut self,
        foreign_proposal: &tari_dan_storage::consensus_models::ForeignProposal,
        proposed_in_block: Option<tari_dan_storage::consensus_models::BlockId>,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_delete(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_delete_in_epoch(&mut self, epoch: tari_dan_common_types::Epoch) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_set_status(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        status: tari_dan_storage::consensus_models::ForeignProposalStatus,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_set_proposed_in(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        proposed_in_block: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_clear_proposed_in(&mut self, proposed_in_block: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_send_counters_set(
        &mut self,
        foreign_send_counter: &tari_dan_storage::consensus_models::ForeignSendCounters,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_receive_counters_set(
        &mut self,
        foreign_send_counter: &tari_dan_storage::consensus_models::ForeignReceiveCounters,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_insert(&mut self, transaction: &tari_dan_storage::consensus_models::TransactionRecord) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_update(&mut self, transaction: &tari_dan_storage::consensus_models::TransactionRecord) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_save_all<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::TransactionRecord>>(
        &mut self,
        transaction: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_finalize_all<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::TransactionPoolRecord>>(
        &mut self,
        block_id: tari_dan_storage::consensus_models::BlockId,
        transaction: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_executions_insert_or_ignore(
        &mut self,
        transaction_execution: &tari_dan_storage::consensus_models::BlockTransactionExecution,
    ) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_executions_remove_any_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_insert_new(
        &mut self,
        tx_id: tari_transaction::TransactionId,
        decision: tari_dan_storage::consensus_models::Decision,
        is_ready: bool,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_add_pending_update(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        pool_update: &tari_dan_storage::consensus_models::TransactionPoolStatusUpdate,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_remove(&mut self, transaction_id: &tari_transaction::TransactionId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_remove_all<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionPoolRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_confirm_all_transitions(&mut self, new_locked_block: &tari_dan_storage::consensus_models::LockedBlock) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_state_updates_remove_any_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId)
        -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn missing_transactions_insert<'a, IMissing: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        park_block: &tari_dan_storage::consensus_models::Block,
        foreign_proposals: &[tari_dan_storage::consensus_models::ForeignProposal],
        missing_transaction_ids: IMissing,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn missing_transactions_remove(
        &mut self,
        height: tari_dan_common_types::NodeHeight,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Option<(tari_dan_storage::consensus_models::Block, Vec<tari_dan_storage::consensus_models::ForeignProposal>)>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_parked_blocks_insert(&mut self, park_block: &tari_dan_storage::consensus_models::ForeignParkedProposal) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_parked_blocks_insert_missing_transactions<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        park_block_id: &tari_dan_storage::consensus_models::BlockId,
        missing_transaction_ids: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_parked_blocks_remove_all_by_transaction(
        &mut self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignParkedProposal>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn votes_insert(&mut self, vote: &tari_dan_storage::consensus_models::Vote) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn votes_delete_all(&mut self) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substate_locks_insert_all<'a, I: IntoIterator<Item = (&'a tari_engine_types::substate::SubstateId, &'a Vec<tari_dan_storage::consensus_models::SubstateLock>)>>(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        locks: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substate_locks_remove_many_for_transactions<'a, I: Iterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        transaction_ids: std::iter::Peekable<I>,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substate_locks_remove_any_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_create(&mut self, substate: &tari_dan_storage::consensus_models::SubstateRecord) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_down(
        &mut self,
        versioned_substate_id: tari_dan_common_types::VersionedSubstateId,
        shard: tari_dan_common_types::shard::Shard,
        epoch: tari_dan_common_types::Epoch,
        destroyed_block_height: tari_dan_common_types::NodeHeight,
        destroyed_transaction_id: &tari_transaction::TransactionId,
        destroyed_qc_id: &tari_dan_storage::consensus_models::QcId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_substate_pledges_save(
        &mut self,
        transaction_id: &tari_transaction::TransactionId,
        shard_group: tari_dan_common_types::ShardGroup,
        pledges: &tari_dan_storage::consensus_models::SubstatePledges,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_substate_pledges_remove_many<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn pending_state_tree_diffs_insert(
        &mut self,
        block_id: tari_dan_storage::consensus_models::BlockId,
        shard: tari_dan_common_types::shard::Shard,
        diff: &tari_dan_storage::consensus_models::VersionedStateHashTreeDiff,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn pending_state_tree_diffs_remove_by_block(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn pending_state_tree_diffs_remove_and_return_by_block(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<indexmap::IndexMap<tari_dan_common_types::shard::Shard, Vec<tari_dan_storage::consensus_models::PendingShardStateTreeDiff>>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_tree_nodes_insert(&mut self, shard: tari_dan_common_types::shard::Shard, key: NodeKey, node: Node<Version>) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_tree_nodes_record_stale_tree_node(
        &mut self,
        shard: tari_dan_common_types::shard::Shard,
        node: StaleTreeNode,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_tree_shard_versions_set(&mut self, shard: tari_dan_common_types::shard::Shard, version: Version) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn epoch_checkpoint_save(&mut self, checkpoint: &tari_dan_storage::consensus_models::EpochCheckpoint) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_insert(&mut self, burnt_utxo: &tari_dan_storage::consensus_models::BurntUtxo) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_set_proposed_block(
        &mut self,
        substate_id: &tari_engine_types::substate::SubstateId,
        proposed_in_block: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_clear_proposed_block(&mut self, proposed_in_block: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_delete(&mut self, substate_id: &tari_engine_types::substate::SubstateId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn lock_conflicts_insert_all<'a, I: IntoIterator<Item = (&'a tari_transaction::TransactionId, &'a Vec<tari_dan_storage::consensus_models::LockConflict>)>>(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        conflicts: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn validator_epoch_stats_add_participation_share(&mut self, qc_id: &tari_dan_storage::consensus_models::QcId) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn validator_epoch_stats_updates<'a, I: IntoIterator<Item = tari_dan_storage::consensus_models::ValidatorStatsUpdate<'a>>>(
        &mut self,
        epoch: tari_dan_common_types::Epoch,
        updates: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn suspended_nodes_insert(
        &mut self,
        public_key: &tari_common_types::types::PublicKey,
        suspended_in_block: tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn suspended_nodes_mark_for_removal(
        &mut self,
        public_key: &tari_common_types::types::PublicKey,
        resumed_in_block: tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn suspended_nodes_delete(&mut self, public_key: &tari_common_types::types::PublicKey) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn diagnostics_add_no_vote(&mut self, block_id: tari_dan_storage::consensus_models::BlockId, reason: tari_dan_storage::consensus_models::NoVoteReason) -> Result<(), tari_dan_storage::StorageError> {
        todo!()
    }
}


pub enum ValidatorNodeStateStoreReadTransaction<'a> {
    Rocksdb(<RocksDbStateStore<PeerAddress> as StateStore>::ReadTransaction<'a>),
    Sqlite(<SqliteStateStore<PeerAddress> as StateStore>::ReadTransaction<'a>),
}

impl<'tx> StateStoreReadTransaction
    for ValidatorNodeStateStoreReadTransaction<'tx>
{
    type Addr = PeerAddress;
    
    fn last_sent_vote_get(&self) -> Result<tari_dan_storage::consensus_models::LastSentVote, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.last_sent_vote_get(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.last_sent_vote_get(),
        }
    }
    
    fn last_voted_get(&self) -> Result<tari_dan_storage::consensus_models::LastVoted, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_executed_get(&self) -> Result<tari_dan_storage::consensus_models::LastExecuted, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn last_proposed_get(&self) -> Result<tari_dan_storage::consensus_models::LastProposed, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn locked_block_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::LockedBlock, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn leaf_block_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::LeafBlock, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn high_qc_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::HighQc, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_get_any<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::BlockId>>(
        &self,
        block_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignProposal>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_exists(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_has_unconfirmed(&self, epoch: tari_dan_common_types::Epoch) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposals_get_all_new(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        limit: usize,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignProposal>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_proposal_get_all_pending(
        &self,
        from_block_id: &tari_dan_storage::consensus_models::BlockId,
        to_block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignProposalAtom>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_send_counters_get(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::ForeignSendCounters, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_receive_counters_get(&self) -> Result<tari_dan_storage::consensus_models::ForeignReceiveCounters, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_get(&self, tx_id: &tari_transaction::TransactionId) -> Result<tari_dan_storage::consensus_models::TransactionRecord, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_exists(&self, tx_id: &tari_transaction::TransactionId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_get_any<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &self,
        tx_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_get_paginated(
        &self,
        limit: u64,
        offset: u64,
        asc_desc_created_at: Option<tari_dan_storage::Ordering>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_executions_get(
        &self,
        tx_id: &tari_transaction::TransactionId,
        block: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<tari_dan_storage::consensus_models::BlockTransactionExecution, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_executions_get_pending_for_block(
        &self,
        tx_id: &tari_transaction::TransactionId,
        from_block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<tari_dan_storage::consensus_models::BlockTransactionExecution, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::Block, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_all_ids_by_height(&self, epoch: tari_dan_common_types::Epoch, height: tari_dan_common_types::NodeHeight) -> Result<Vec<tari_dan_storage::consensus_models::BlockId>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_genesis_for_epoch(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::Block, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_last_n_in_epoch(&self, n: usize, epoch: tari_dan_common_types::Epoch) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_all_between(
        &self,
        epoch: tari_dan_common_types::Epoch,
        shard_group: tari_dan_common_types::ShardGroup,
        start_block_id: &tari_dan_storage::consensus_models::BlockId,
        end_block_id: &tari_dan_storage::consensus_models::BlockId,
        include_dummy_blocks: bool,
    ) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_exists(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_is_ancestor(&self, descendant: &tari_dan_storage::consensus_models::BlockId, ancestor: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_all_by_parent(&self, parent: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_ids_by_parent(&self, parent: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_dan_storage::consensus_models::BlockId>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_parent_chain(&self, block_id: &tari_dan_storage::consensus_models::BlockId, limit: usize) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_pending_transactions(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_transaction::TransactionId>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_total_leader_fee_for_epoch(
        &self,
        epoch: tari_dan_common_types::Epoch,
        validator_public_key: &tari_common_types::types::PublicKey,
    ) -> Result<u64, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_any_with_epoch_range(
        &self,
        epoch_range: std::ops::RangeInclusive<tari_dan_common_types::Epoch>,
        validator_public_key: Option<&tari_common_types::types::PublicKey>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_paginated(
        &self,
        limit: u64,
        offset: u64,
        filter_index: Option<usize>,
        filter: Option<String>,
        ordering_index: Option<usize>,
        ordering: Option<tari_dan_storage::Ordering>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_get_count(&self) -> Result<i64, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn filtered_blocks_get_count(
        &self,
        filter_index: Option<usize>,
        filter: Option<String>,
    ) -> Result<i64, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn blocks_max_height(&self) -> Result<tari_dan_common_types::NodeHeight, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn block_diffs_get(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::BlockDiff, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn block_diffs_get_last_change_for_substate(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        substate_id: &tari_engine_types::substate::SubstateId,
    ) -> Result<tari_dan_storage::consensus_models::SubstateChange, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn quorum_certificates_get(&self, qc_id: &tari_dan_storage::consensus_models::QcId) -> Result<tari_dan_storage::consensus_models::QuorumCertificate, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn quorum_certificates_get_all<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::QcId>>(
        &self,
        qc_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::QuorumCertificate>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn quorum_certificates_get_by_block_id(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::QuorumCertificate, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_get_for_blocks(
        &self,
        from_block_id: &tari_dan_storage::consensus_models::BlockId,
        to_block_id: &tari_dan_storage::consensus_models::BlockId,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<tari_dan_storage::consensus_models::TransactionPoolRecord, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_exists(&self, transaction_id: &tari_transaction::TransactionId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_get_all(&self) -> Result<Vec<tari_dan_storage::consensus_models::TransactionPoolRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_get_many_ready(
        &self,
        max_txs: usize,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionPoolRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transaction_pool_count(
        &self,
        stage: Option<tari_dan_storage::consensus_models::TransactionPoolStage>,
        is_ready: Option<bool>,
        confirmed_stage: Option<Option<tari_dan_storage::consensus_models::TransactionPoolConfirmedStage>>,
    ) -> Result<usize, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn transactions_fetch_involved_shards(
        &self,
        transaction_ids: std::collections::HashSet<tari_transaction::TransactionId>,
    ) -> Result<std::collections::HashSet<tari_dan_common_types::SubstateAddress>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn votes_get_by_block_and_sender(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        sender_leaf_hash: &tari_common_types::types::FixedHash,
    ) -> Result<tari_dan_storage::consensus_models::Vote, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn votes_count_for_block(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<u64, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn votes_get_for_block(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_dan_storage::consensus_models::Vote>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get(&self, address: &tari_dan_common_types::SubstateAddress) -> Result<tari_dan_storage::consensus_models::SubstateRecord, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_any(
        &self,
        substate_ids: &std::collections::HashSet<tari_dan_common_types::SubstateRequirement>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_any_max_version<'a, I: IntoIterator<Item = &'a tari_engine_types::substate::SubstateId>>(
        &self,
        substate_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_max_version_for_substate(&self, substate_id: &tari_engine_types::substate::SubstateId) -> Result<(u32, bool), tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_any_exist<I, S>(&self, substates: I) -> Result<bool, tari_dan_storage::StorageError>
    where
        I: IntoIterator<Item = S>,
        S: std::borrow::Borrow<tari_dan_common_types::VersionedSubstateId> {
        todo!()
    }
    
    fn substates_exists_for_transaction(&self, transaction_id: &tari_transaction::TransactionId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_n_after(&self, n: usize, after: &tari_dan_common_types::SubstateAddress) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_many_within_range(
        &self,
        start: &tari_dan_common_types::SubstateAddress,
        end: &tari_dan_common_types::SubstateAddress,
        exclude_shards: &[tari_dan_common_types::SubstateAddress],
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_many_by_created_transaction(
        &self,
        tx_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_many_by_destroyed_transaction(
        &self,
        tx_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substates_get_all_for_transaction(
        &self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substate_locks_get_locked_substates_for_transaction(
        &self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::LockedSubstateValue>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn substate_locks_get_latest_for_substate(&self, substate_id: &tari_engine_types::substate::SubstateId) -> Result<tari_dan_storage::consensus_models::SubstateLock, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn pending_state_tree_diffs_get_all_up_to_commit_block(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<std::collections::HashMap<tari_dan_common_types::shard::Shard, Vec<tari_dan_storage::consensus_models::PendingShardStateTreeDiff>>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_transitions_get_n_after(
        &self,
        n: usize,
        id: tari_dan_storage::consensus_models::StateTransitionId,
        end_epoch: tari_dan_common_types::Epoch,
    ) -> Result<Vec<tari_dan_storage::consensus_models::StateTransition>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_transitions_get_last_id(&self, shard: tari_dan_common_types::shard::Shard) -> Result<tari_dan_storage::consensus_models::StateTransitionId, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_tree_nodes_get(&self, shard: tari_dan_common_types::shard::Shard, key: &NodeKey) -> Result<Node<Version>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn state_tree_versions_get_latest(&self, shard: tari_dan_common_types::shard::Shard) -> Result<Option<Version>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn epoch_checkpoint_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::EpochCheckpoint, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_substate_pledges_exists_for_address<T: tari_dan_common_types::ToSubstateAddress>(
        &self,
        transaction_id: &tari_transaction::TransactionId,
        address: T,
    ) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_substate_pledges_get_all_by_transaction_id(
        &self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<tari_dan_storage::consensus_models::SubstatePledges, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_get(&self, substate_id: &tari_engine_types::substate::SubstateId) -> Result<tari_dan_storage::consensus_models::BurntUtxo, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_get_all_unproposed(
        &self,
        leaf_block: &tari_dan_storage::consensus_models::BlockId,
        limit: usize,
    ) -> Result<Vec<tari_dan_storage::consensus_models::BurntUtxo>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn burnt_utxos_count(&self) -> Result<u64, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn foreign_parked_blocks_exists(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn validator_epoch_stats_get(
        &self,
        epoch: tari_dan_common_types::Epoch,
        public_key: &tari_common_types::types::PublicKey,
    ) -> Result<tari_dan_storage::consensus_models::ValidatorConsensusStats, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn validator_epoch_stats_get_nodes_to_suspend(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        min_missed_proposals: u64,
        limit: usize,
    ) -> Result<Vec<tari_common_types::types::PublicKey>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn validator_epoch_stats_get_nodes_to_resume(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        limit: usize,
    ) -> Result<Vec<tari_common_types::types::PublicKey>, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn suspended_nodes_is_suspended(&self, block_id: &tari_dan_storage::consensus_models::BlockId, public_key: &tari_common_types::types::PublicKey) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }
    
    fn suspended_nodes_count(&self) -> Result<u64, tari_dan_storage::StorageError> {
        todo!()
    }


}