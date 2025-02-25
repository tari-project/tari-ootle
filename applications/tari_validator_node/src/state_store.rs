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
use tari_dan_common_types::PeerAddress;
use tari_dan_storage::consensus_models::{Decision, Evidence};
use tari_dan_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_state_tree::{Node, NodeKey, StaleTreeNode, Version};
use tari_dan_common_types::NodeHeight;
use tari_dan_common_types::ShardGroup;
use tari_dan_common_types::Epoch;
use tari_template_lib::models::UnclaimedConfidentialOutputAddress;
use tari_transaction::TransactionId;

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
                let read_tx = ValidatorNodeStateStoreReadTransaction::Rocksdb(tx);
                let write_tx = db.create_write_tx()?;
                Ok(ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, read_tx } )
            },
            ValidatorNodeStateStore::Sqlite(db) => {
                let tx = db.create_read_tx()?;
                let read_tx = ValidatorNodeStateStoreReadTransaction::Sqlite(tx);
                let write_tx = db.create_write_tx()?;
                Ok(ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, read_tx })
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

pub enum ValidatorNodeStateStoreWriteTransaction<'a> {
    Rocksdb {
        write_tx: <RocksDbStateStore<PeerAddress> as StateStore>::WriteTransaction<'a>,
        read_tx: ValidatorNodeStateStoreReadTransaction<'a>,
    },
    Sqlite {
        write_tx: <SqliteStateStore<PeerAddress> as StateStore>::WriteTransaction<'a>,
        read_tx: ValidatorNodeStateStoreReadTransaction<'a>,
    },
}

impl<'a> Deref for ValidatorNodeStateStoreWriteTransaction<'a> {
    type Target = ValidatorNodeStateStoreReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { read_tx, .. } => read_tx,
            ValidatorNodeStateStoreWriteTransaction::Sqlite { read_tx, .. } => read_tx,
        }
    }
}


impl<'tx> StateStoreWriteTransaction
    for ValidatorNodeStateStoreWriteTransaction<'tx>
{
    type Addr = PeerAddress;
    
    fn commit(&mut self) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.commit(),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.commit(),
        }
    }
    
    fn rollback(&mut self) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.rollback(),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.rollback(),
        }
    }
    
    fn blocks_insert(&mut self, block: &tari_dan_storage::consensus_models::Block) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.blocks_insert(block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.blocks_insert(block),
        }
    }
    
    fn blocks_delete(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.blocks_delete(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.blocks_delete(block_id),
        }
    }
    
    fn blocks_set_flags(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        is_committed: Option<bool>,
        is_justified: Option<bool>,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.blocks_set_flags(block_id, is_committed, is_justified),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.blocks_set_flags(block_id, is_committed, is_justified),
        }
    }
    
    fn block_diffs_insert(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId, changes: &[tari_dan_storage::consensus_models::SubstateChange]) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.block_diffs_insert(block_id, changes),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.block_diffs_insert(block_id, changes),
        }
    }
    
    fn block_diffs_remove(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.block_diffs_remove(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.block_diffs_remove(block_id),
        }
    }
    
    fn quorum_certificates_insert(&mut self, qc: &tari_dan_storage::consensus_models::QuorumCertificate) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.quorum_certificates_insert(qc),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.quorum_certificates_insert(qc),
        }
    }
    
    fn quorum_certificates_set_shares_processed(&mut self, qc_id: &tari_dan_storage::consensus_models::QcId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.quorum_certificates_set_shares_processed(qc_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.quorum_certificates_set_shares_processed(qc_id),
        }
    }
    
    fn last_sent_vote_set(&mut self, last_sent_vote: &tari_dan_storage::consensus_models::LastSentVote) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_sent_vote_set(last_sent_vote),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.last_sent_vote_set(last_sent_vote),
        }
    }
    
    fn last_voted_set(&mut self, last_voted: &tari_dan_storage::consensus_models::LastVoted) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_voted_set(last_voted),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.last_voted_set(last_voted),
        }
    }
    
    fn last_votes_unset(&mut self, last_voted: &tari_dan_storage::consensus_models::LastVoted) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_votes_unset(last_voted),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.last_votes_unset(last_voted),
        }
    }
    
    fn last_executed_set(&mut self, last_exec: &tari_dan_storage::consensus_models::LastExecuted) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_executed_set(last_exec),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.last_executed_set(last_exec),
        }
    }
    
    fn last_proposed_set(&mut self, last_proposed: &tari_dan_storage::consensus_models::LastProposed) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_proposed_set(last_proposed),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.last_proposed_set(last_proposed),
        }
    }
    
    fn last_proposed_unset(&mut self, last_proposed: &tari_dan_storage::consensus_models::LastProposed) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.last_proposed_unset(last_proposed),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.last_proposed_unset(last_proposed),
        }
    }
    
    fn leaf_block_set(&mut self, leaf_node: &tari_dan_storage::consensus_models::LeafBlock) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.leaf_block_set(leaf_node),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.leaf_block_set(leaf_node),
        }
    }
    
    fn locked_block_set(&mut self, locked_block: &tari_dan_storage::consensus_models::LockedBlock) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.locked_block_set(locked_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.locked_block_set(locked_block),
        }
    }
    
    fn high_qc_set(&mut self, high_qc: &tari_dan_storage::consensus_models::HighQc) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.high_qc_set(high_qc),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.high_qc_set(high_qc),
        }
    }
    
    fn foreign_proposals_upsert(
        &mut self,
        foreign_proposal: &tari_dan_storage::consensus_models::ForeignProposal,
        proposed_in_block: Option<tari_dan_storage::consensus_models::BlockId>,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_upsert(foreign_proposal, proposed_in_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_proposals_upsert(foreign_proposal, proposed_in_block),
        }
    }
    
    fn foreign_proposals_delete(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_delete(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_proposals_delete(block_id),
        }
    }
    
    fn foreign_proposals_delete_in_epoch(&mut self, epoch: tari_dan_common_types::Epoch) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_delete_in_epoch(epoch),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_proposals_delete_in_epoch(epoch),
        }
    }
    
    fn foreign_proposals_set_status(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        status: tari_dan_storage::consensus_models::ForeignProposalStatus,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_set_status(block_id, status),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_proposals_set_status(block_id, status),
        }
    }
    
    fn foreign_proposals_set_proposed_in(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        proposed_in_block: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_set_proposed_in(block_id, proposed_in_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_proposals_set_proposed_in(block_id, proposed_in_block),
        }
    }
    
    fn foreign_proposals_clear_proposed_in(&mut self, proposed_in_block: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_proposals_clear_proposed_in(proposed_in_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_proposals_clear_proposed_in(proposed_in_block),
        }
    }
    
    fn foreign_send_counters_set(
        &mut self,
        foreign_send_counter: &tari_dan_storage::consensus_models::ForeignSendCounters,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_send_counters_set(foreign_send_counter, block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_send_counters_set(foreign_send_counter, block_id),
        }
    }
    
    fn foreign_receive_counters_set(
        &mut self,
        foreign_send_counter: &tari_dan_storage::consensus_models::ForeignReceiveCounters,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_receive_counters_set(foreign_send_counter),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_receive_counters_set(foreign_send_counter),
        }
    }
    
    fn transactions_insert(&mut self, transaction: &tari_dan_storage::consensus_models::TransactionRecord) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transactions_insert(transaction),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transactions_insert(transaction),
        }
    }
    
    fn transactions_update(&mut self, transaction: &tari_dan_storage::consensus_models::TransactionRecord) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transactions_update(transaction),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transactions_update(transaction),
        }
    }
    
    fn transactions_save_all<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::TransactionRecord>>(
        &mut self,
        transaction: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transactions_save_all(transaction),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transactions_save_all(transaction),
        }
    }
    
    fn transactions_finalize_all<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::TransactionPoolRecord>>(
        &mut self,
        block_id: tari_dan_storage::consensus_models::BlockId,
        transaction: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transactions_finalize_all(block_id, transaction),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transactions_finalize_all(block_id, transaction),
        }
    }
    
    fn transaction_executions_insert_or_ignore(
        &mut self,
        transaction_execution: &tari_dan_storage::consensus_models::BlockTransactionExecution,
    ) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_executions_insert_or_ignore(transaction_execution),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_executions_insert_or_ignore(transaction_execution),
        }
    }
    
    fn transaction_executions_remove_any_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_executions_remove_any_by_block_id(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_executions_remove_any_by_block_id(block_id),
        }
    }

    fn transaction_pool_insert_new(
        &mut self,
        tx_id: TransactionId,
        decision: Decision,
        initial_evidence: &Evidence,
        is_ready: bool,
        is_global: bool,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_pool_insert_new(tx_id, decision, initial_evidence, is_ready, is_global),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_pool_insert_new(tx_id, decision, initial_evidence, is_ready, is_global),
        }
    }
    
    fn transaction_pool_add_pending_update(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        pool_update: &tari_dan_storage::consensus_models::TransactionPoolStatusUpdate,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_pool_add_pending_update(block_id, pool_update),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_pool_add_pending_update(block_id, pool_update),
        }
    }
    
    fn transaction_pool_remove(&mut self, transaction_id: &tari_transaction::TransactionId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_pool_remove(transaction_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_pool_remove(transaction_id),
        }
    }
    
    fn transaction_pool_remove_all<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionPoolRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_pool_remove_all(transaction_ids),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_pool_remove_all(transaction_ids),
        }
    }
    
    fn transaction_pool_confirm_all_transitions(&mut self, new_locked_block: &tari_dan_storage::consensus_models::LockedBlock) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_pool_confirm_all_transitions(new_locked_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_pool_confirm_all_transitions(new_locked_block),
        }
    }
    
    fn transaction_pool_state_updates_remove_any_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId)
        -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.transaction_pool_state_updates_remove_any_by_block_id(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.transaction_pool_state_updates_remove_any_by_block_id(block_id),
        }
    }
    
    fn missing_transactions_insert<'a, IMissing: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        park_block: &tari_dan_storage::consensus_models::Block,
        foreign_proposals: &[tari_dan_storage::consensus_models::ForeignProposal],
        missing_transaction_ids: IMissing,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.missing_transactions_insert(park_block, foreign_proposals, missing_transaction_ids),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.missing_transactions_insert(park_block, foreign_proposals, missing_transaction_ids),
        }
    }
    
    fn missing_transactions_remove(
        &mut self,
        height: tari_dan_common_types::NodeHeight,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Option<(tari_dan_storage::consensus_models::Block, Vec<tari_dan_storage::consensus_models::ForeignProposal>)>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.missing_transactions_remove(height, transaction_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.missing_transactions_remove(height, transaction_id),
        }
    }
    
    fn foreign_parked_blocks_insert(&mut self, park_block: &tari_dan_storage::consensus_models::ForeignParkedProposal) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_parked_blocks_insert(park_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_parked_blocks_insert(park_block),
        }
    }
    
    fn foreign_parked_blocks_insert_missing_transactions<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        park_block_id: &tari_dan_storage::consensus_models::BlockId,
        missing_transaction_ids: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_parked_blocks_insert_missing_transactions(park_block_id, missing_transaction_ids),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_parked_blocks_insert_missing_transactions(park_block_id, missing_transaction_ids),
        }
    }
    
    fn foreign_parked_blocks_remove_all_by_transaction(
        &mut self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignParkedProposal>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_parked_blocks_remove_all_by_transaction(transaction_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_parked_blocks_remove_all_by_transaction(transaction_id),
        }
    }
    
    fn votes_insert(&mut self, vote: &tari_dan_storage::consensus_models::Vote) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.votes_insert(vote),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.votes_insert(vote),
        }
    }
    
    fn votes_delete_all(&mut self) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.votes_delete_all(),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.votes_delete_all(),
        }
    }
    
    fn substate_locks_insert_all<'a, I: IntoIterator<Item = (&'a tari_engine_types::substate::SubstateId, &'a Vec<tari_dan_storage::consensus_models::SubstateLock>)>>(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        locks: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substate_locks_insert_all(block_id, locks),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.substate_locks_insert_all(block_id, locks),
        }
    }
    
    fn substate_locks_remove_many_for_transactions<'a, I: Iterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        transaction_ids: std::iter::Peekable<I>,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substate_locks_remove_many_for_transactions(transaction_ids),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.substate_locks_remove_many_for_transactions(transaction_ids),
        }
    }
    
    fn substate_locks_remove_any_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substate_locks_remove_any_by_block_id(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.substate_locks_remove_any_by_block_id(block_id),
        }
    }
    
    fn substates_create(&mut self, substate: &tari_dan_storage::consensus_models::SubstateRecord) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substates_create(substate),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.substates_create(substate),
        }
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
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.substates_down(versioned_substate_id, shard, epoch, destroyed_block_height, destroyed_transaction_id, destroyed_qc_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.substates_down(versioned_substate_id, shard, epoch, destroyed_block_height, destroyed_transaction_id, destroyed_qc_id),
        }
    }
    
    fn foreign_substate_pledges_save(
        &mut self,
        transaction_id: &tari_transaction::TransactionId,
        shard_group: tari_dan_common_types::ShardGroup,
        pledges: &tari_dan_storage::consensus_models::SubstatePledges,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_substate_pledges_save(transaction_id, shard_group, pledges),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_substate_pledges_save(transaction_id, shard_group, pledges),
        }
    }
    
    fn foreign_substate_pledges_remove_many<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.foreign_substate_pledges_remove_many(transaction_ids),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.foreign_substate_pledges_remove_many(transaction_ids),
        }
    }
    
    fn pending_state_tree_diffs_insert(
        &mut self,
        block_id: tari_dan_storage::consensus_models::BlockId,
        shard: tari_dan_common_types::shard::Shard,
        diff: &tari_dan_storage::consensus_models::VersionedStateHashTreeDiff,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.pending_state_tree_diffs_insert(block_id, shard, diff),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.pending_state_tree_diffs_insert(block_id, shard, diff),
        }
    }
    
    fn pending_state_tree_diffs_remove_by_block(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.pending_state_tree_diffs_remove_by_block(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.pending_state_tree_diffs_remove_by_block(block_id),
        }
    }
    
    fn pending_state_tree_diffs_remove_and_return_by_block(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<indexmap::IndexMap<tari_dan_common_types::shard::Shard, Vec<tari_dan_storage::consensus_models::PendingShardStateTreeDiff>>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.pending_state_tree_diffs_remove_and_return_by_block(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.pending_state_tree_diffs_remove_and_return_by_block(block_id),
        }
    }
    
    fn state_tree_nodes_insert(&mut self, shard: tari_dan_common_types::shard::Shard, key: NodeKey, node: Node<Version>) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.state_tree_nodes_insert(shard, key, node),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.state_tree_nodes_insert(shard, key, node),
        }
    }
    
    fn state_tree_nodes_record_stale_tree_node(
        &mut self,
        shard: tari_dan_common_types::shard::Shard,
        node: StaleTreeNode,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.state_tree_nodes_record_stale_tree_node(shard, node),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.state_tree_nodes_record_stale_tree_node(shard, node),
        }
    }
    
    fn state_tree_shard_versions_set(&mut self, shard: tari_dan_common_types::shard::Shard, version: Version) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.state_tree_shard_versions_set(shard, version),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.state_tree_shard_versions_set(shard, version),
        }
    }
    
    fn epoch_checkpoint_save(&mut self, checkpoint: &tari_dan_storage::consensus_models::EpochCheckpoint) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.epoch_checkpoint_save(checkpoint),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.epoch_checkpoint_save(checkpoint),
        }
    }
    
    fn burnt_utxos_insert(&mut self, burnt_utxo: &tari_dan_storage::consensus_models::BurntUtxo) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.burnt_utxos_insert(burnt_utxo),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.burnt_utxos_insert(burnt_utxo),
        }
    }
    
    fn burnt_utxos_set_proposed_block(
        &mut self,
        commitment: &UnclaimedConfidentialOutputAddress,
        proposed_in_block: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.burnt_utxos_set_proposed_block(commitment, proposed_in_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.burnt_utxos_set_proposed_block(commitment, proposed_in_block),
        }
    }
    
    fn burnt_utxos_clear_proposed_block(&mut self, proposed_in_block: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.burnt_utxos_clear_proposed_block(proposed_in_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.burnt_utxos_clear_proposed_block(proposed_in_block),
        }
    }
    
    fn burnt_utxos_delete(&mut self, commitment: &UnclaimedConfidentialOutputAddress) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.burnt_utxos_delete(commitment),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.burnt_utxos_delete(commitment),
        }
    }
    
    fn lock_conflicts_insert_all<'a, I: IntoIterator<Item = (&'a tari_transaction::TransactionId, &'a Vec<tari_dan_storage::consensus_models::LockConflict>)>>(
        &mut self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        conflicts: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.lock_conflicts_insert_all(block_id, conflicts),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.lock_conflicts_insert_all(block_id, conflicts),
        }
    }
    
    fn validator_epoch_stats_add_participation_share(&mut self, qc_id: &tari_dan_storage::consensus_models::QcId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.validator_epoch_stats_add_participation_share(qc_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.validator_epoch_stats_add_participation_share(qc_id),
        }
    }
    
    fn validator_epoch_stats_updates<'a, I: IntoIterator<Item = tari_dan_storage::consensus_models::ValidatorStatsUpdate<'a>>>(
        &mut self,
        epoch: tari_dan_common_types::Epoch,
        updates: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.validator_epoch_stats_updates(epoch, updates),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.validator_epoch_stats_updates(epoch, updates),
        }
    }
    
    fn diagnostics_add_no_vote(&mut self, block_id: tari_dan_storage::consensus_models::BlockId, reason: tari_dan_storage::consensus_models::NoVoteReason) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.diagnostics_add_no_vote(block_id, reason),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.diagnostics_add_no_vote(block_id, reason),
        }
    }
    
    fn lock_conflicts_remove_by_transaction_ids<'a, I: IntoIterator<Item = &'a TransactionId>>(
        &mut self,
        transaction_ids: I,
    ) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.lock_conflicts_remove_by_transaction_ids(transaction_ids),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.lock_conflicts_remove_by_transaction_ids(transaction_ids),
        }
    }
    
    fn lock_conflicts_remove_by_block_id(&mut self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.lock_conflicts_remove_by_block_id(block_id),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.lock_conflicts_remove_by_block_id(block_id),
        }
    }
    
    fn evicted_nodes_evict(&mut self, public_key: &tari_common_types::types::PublicKey, evicted_in_block: tari_dan_storage::consensus_models::BlockId) -> Result<(), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreWriteTransaction::Rocksdb { write_tx, .. } => write_tx.evicted_nodes_evict(public_key, evicted_in_block),
            ValidatorNodeStateStoreWriteTransaction::Sqlite { write_tx, .. }  => write_tx.evicted_nodes_evict(public_key, evicted_in_block),
        }
    }
    
    fn evicted_nodes_mark_eviction_as_committed(
        &mut self,
        public_key: &tari_common_types::types::PublicKey,
        epoch: Epoch,
    ) -> Result<(), tari_dan_storage::StorageError> {
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
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.last_voted_get(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.last_voted_get(),
        }
    }
    
    fn last_executed_get(&self) -> Result<tari_dan_storage::consensus_models::LastExecuted, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.last_executed_get(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.last_executed_get(),
        }
    }
    
    fn last_proposed_get(&self) -> Result<tari_dan_storage::consensus_models::LastProposed, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.last_proposed_get(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.last_proposed_get(),
        }
    }
    
    fn locked_block_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::LockedBlock, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.locked_block_get(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.locked_block_get(epoch),
        }
    }
    
    fn leaf_block_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::LeafBlock, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.leaf_block_get(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.leaf_block_get(epoch),
        }
    }
    
    fn high_qc_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::HighQc, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.high_qc_get(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.high_qc_get(epoch),
        }
    }
    
    fn foreign_proposals_get_any<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::BlockId>>(
        &self,
        block_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignProposal>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_proposals_get_any(block_ids),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_proposals_get_any(block_ids),
        }
    }
    
    fn foreign_proposals_exists(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_proposals_exists(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_proposals_exists(block_id),
        }
    }
    
    fn foreign_proposals_has_unconfirmed(&self, epoch: tari_dan_common_types::Epoch) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_proposals_has_unconfirmed(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_proposals_has_unconfirmed(epoch),
        }
    }
    
    fn foreign_proposals_get_all_new(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        limit: usize,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignProposal>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_proposals_get_all_new(block_id, limit),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_proposals_get_all_new(block_id, limit),
        }
    }
    
    fn foreign_proposal_get_all_pending(
        &self,
        from_block_id: &tari_dan_storage::consensus_models::BlockId,
        to_block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::ForeignProposalAtom>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_proposal_get_all_pending(from_block_id, to_block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_proposal_get_all_pending(from_block_id, to_block_id),
        }
    }
    
    fn foreign_send_counters_get(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::ForeignSendCounters, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_send_counters_get(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_send_counters_get(block_id),
        }
    }
    
    fn foreign_receive_counters_get(&self) -> Result<tari_dan_storage::consensus_models::ForeignReceiveCounters, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_receive_counters_get(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_receive_counters_get(),
        }
    }
    
    fn transactions_get(&self, tx_id: &tari_transaction::TransactionId) -> Result<tari_dan_storage::consensus_models::TransactionRecord, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transactions_get(tx_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transactions_get(tx_id),
        }
    }
    
    fn transactions_exists(&self, tx_id: &tari_transaction::TransactionId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transactions_exists(tx_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transactions_exists(tx_id),
        }
    }
    
    fn transactions_get_any<'a, I: IntoIterator<Item = &'a tari_transaction::TransactionId>>(
        &self,
        tx_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transactions_get_any(tx_ids),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transactions_get_any(tx_ids),
        }
    }
    
    fn transactions_get_paginated(
        &self,
        limit: u64,
        offset: u64,
        asc_desc_created_at: Option<tari_dan_storage::Ordering>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transactions_get_paginated(limit, offset, asc_desc_created_at),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transactions_get_paginated(limit, offset, asc_desc_created_at),
        }
    }
    
    fn transaction_executions_get(
        &self,
        tx_id: &tari_transaction::TransactionId,
        block: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<tari_dan_storage::consensus_models::BlockTransactionExecution, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_executions_get(tx_id, block),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_executions_get(tx_id, block),
        }
    }
    
    fn transaction_executions_get_pending_for_block(
        &self,
        tx_id: &tari_transaction::TransactionId,
        from_block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<tari_dan_storage::consensus_models::BlockTransactionExecution, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_executions_get_pending_for_block(tx_id, from_block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_executions_get_pending_for_block(tx_id, from_block_id),
        }
    }
    
    fn blocks_get(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::Block, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get(block_id),
        }
    }
    
    fn blocks_get_all_ids_by_height(&self, epoch: tari_dan_common_types::Epoch, height: tari_dan_common_types::NodeHeight) -> Result<Vec<tari_dan_storage::consensus_models::BlockId>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_all_ids_by_height(epoch, height),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_all_ids_by_height(epoch, height),
        }
    }
    
    fn blocks_get_genesis_for_epoch(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::Block, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_genesis_for_epoch(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_genesis_for_epoch(epoch),
        }
    }
    
    fn blocks_get_last_n_in_epoch(&self, n: usize, epoch: tari_dan_common_types::Epoch) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_last_n_in_epoch(n, epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_last_n_in_epoch(n, epoch),
        }
    }
    
    fn blocks_get_all_between(
        &self,
        epoch: Epoch,
        shard_group: ShardGroup,
        start_block_height: NodeHeight,
        end_block_height: NodeHeight,
        include_dummy_blocks: bool,
        limit: u64,
    ) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_all_between(epoch, shard_group, start_block_height, end_block_height, include_dummy_blocks, limit),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_all_between(epoch, shard_group, start_block_height, end_block_height, include_dummy_blocks, limit),
        }
    }
    
    fn blocks_exists(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_exists(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_exists(block_id),
        }
    }
    
    fn blocks_is_ancestor(&self, descendant: &tari_dan_storage::consensus_models::BlockId, ancestor: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_is_ancestor(descendant, ancestor),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_is_ancestor(descendant, ancestor),
        }
    }
    
    fn blocks_get_all_by_parent(&self, parent: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_all_by_parent(parent),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_all_by_parent(parent),
        }
    }
    
    fn blocks_get_ids_by_parent(&self, parent: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_dan_storage::consensus_models::BlockId>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_ids_by_parent(parent),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_ids_by_parent(parent),
        }
    }
    
    fn blocks_get_parent_chain(&self, block_id: &tari_dan_storage::consensus_models::BlockId, limit: usize) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_parent_chain(block_id, limit),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_parent_chain(block_id, limit),
        }
    }
    
    fn blocks_get_pending_transactions(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_transaction::TransactionId>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_pending_transactions(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_pending_transactions(block_id),
        }
    }
    
    fn blocks_get_total_leader_fee_for_epoch(
        &self,
        epoch: tari_dan_common_types::Epoch,
        validator_public_key: &tari_common_types::types::PublicKey,
    ) -> Result<u64, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_total_leader_fee_for_epoch(epoch, validator_public_key),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_total_leader_fee_for_epoch(epoch, validator_public_key),
        }
    }
    
    fn blocks_get_any_with_epoch_range(
        &self,
        epoch_range: std::ops::RangeInclusive<tari_dan_common_types::Epoch>,
        validator_public_key: Option<&tari_common_types::types::PublicKey>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_any_with_epoch_range(epoch_range, validator_public_key),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_any_with_epoch_range(epoch_range, validator_public_key),
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
    ) -> Result<Vec<tari_dan_storage::consensus_models::Block>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_paginated(limit, offset, filter_index, filter, ordering_index, ordering),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_paginated(limit, offset, filter_index, filter, ordering_index, ordering),
        }
    }
    
    fn blocks_get_count(&self) -> Result<i64, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_get_count(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_get_count(),
        }
    }
    
    fn filtered_blocks_get_count(
        &self,
        filter_index: Option<usize>,
        filter: Option<String>,
    ) -> Result<i64, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.filtered_blocks_get_count(filter_index, filter),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.filtered_blocks_get_count(filter_index, filter),
        }
    }
    
    fn blocks_max_height(&self) -> Result<tari_dan_common_types::NodeHeight, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.blocks_max_height(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.blocks_max_height(),
        }
    }
    
    fn block_diffs_get(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::BlockDiff, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.block_diffs_get(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.block_diffs_get(block_id),
        }
    }
    
    fn block_diffs_get_last_change_for_substate(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        substate_id: &tari_engine_types::substate::SubstateId,
    ) -> Result<tari_dan_storage::consensus_models::SubstateChange, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.block_diffs_get_last_change_for_substate(block_id, substate_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.block_diffs_get_last_change_for_substate(block_id, substate_id),
        }
    }
    
    fn quorum_certificates_get(&self, qc_id: &tari_dan_storage::consensus_models::QcId) -> Result<tari_dan_storage::consensus_models::QuorumCertificate, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.quorum_certificates_get(qc_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.quorum_certificates_get(qc_id),
        }
    }
    
    fn quorum_certificates_get_all<'a, I: IntoIterator<Item = &'a tari_dan_storage::consensus_models::QcId>>(
        &self,
        qc_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::QuorumCertificate>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.quorum_certificates_get_all(qc_ids),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.quorum_certificates_get_all(qc_ids),
        }
    }
    
    fn quorum_certificates_get_by_block_id(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<tari_dan_storage::consensus_models::QuorumCertificate, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.quorum_certificates_get_by_block_id(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.quorum_certificates_get_by_block_id(block_id),
        }
    }
    
    fn transaction_pool_get_for_blocks(
        &self,
        from_block_id: &tari_dan_storage::consensus_models::BlockId,
        to_block_id: &tari_dan_storage::consensus_models::BlockId,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<tari_dan_storage::consensus_models::TransactionPoolRecord, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_pool_get_for_blocks(from_block_id, to_block_id, transaction_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_pool_get_for_blocks(from_block_id, to_block_id, transaction_id),
        }
    }
    
    fn transaction_pool_exists(&self, transaction_id: &tari_transaction::TransactionId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_pool_exists(transaction_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_pool_exists(transaction_id),
        }
    }
    
    fn transaction_pool_get_all(&self) -> Result<Vec<tari_dan_storage::consensus_models::TransactionPoolRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_pool_get_all(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_pool_get_all(),
        }
    }
    
    fn transaction_pool_get_many_ready(
        &self,
        max_txs: usize,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::TransactionPoolRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_pool_get_many_ready(max_txs, block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_pool_get_many_ready(max_txs, block_id),
        }
    }
    
    fn transaction_pool_count(
        &self,
        stage: Option<tari_dan_storage::consensus_models::TransactionPoolStage>,
        is_ready: Option<bool>,
        confirmed_stage: Option<Option<tari_dan_storage::consensus_models::TransactionPoolConfirmedStage>>,
    ) -> Result<usize, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transaction_pool_count(stage, is_ready, confirmed_stage),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transaction_pool_count(stage, is_ready, confirmed_stage),
        }
    }
    
    fn transactions_fetch_involved_shards(
        &self,
        transaction_ids: std::collections::HashSet<tari_transaction::TransactionId>,
    ) -> Result<std::collections::HashSet<tari_dan_common_types::SubstateAddress>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.transactions_fetch_involved_shards(transaction_ids),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.transactions_fetch_involved_shards(transaction_ids),
        }
    }
    
    fn votes_get_by_block_and_sender(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        sender_leaf_hash: &tari_common_types::types::FixedHash,
    ) -> Result<tari_dan_storage::consensus_models::Vote, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.votes_get_by_block_and_sender(block_id, sender_leaf_hash),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.votes_get_by_block_and_sender(block_id, sender_leaf_hash),
        }
    }
    
    fn votes_count_for_block(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<u64, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.votes_count_for_block(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.votes_count_for_block(block_id),
        }
    }
    
    fn votes_get_for_block(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<Vec<tari_dan_storage::consensus_models::Vote>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.votes_get_for_block(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.votes_get_for_block(block_id),
        }
    }
    
    fn substates_get(&self, address: &tari_dan_common_types::SubstateAddress) -> Result<tari_dan_storage::consensus_models::SubstateRecord, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get(address),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get(address),
        }
    }
    
    fn substates_get_any(
        &self,
        substate_ids: &std::collections::HashSet<tari_dan_common_types::SubstateRequirement>,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_any(substate_ids),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_any(substate_ids),
        }
    }
    
    fn substates_get_any_max_version<'a, I: IntoIterator<Item = &'a tari_engine_types::substate::SubstateId>>(
        &self,
        substate_ids: I,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_any_max_version(substate_ids),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_any_max_version(substate_ids),
        }
    }
    
    fn substates_get_max_version_for_substate(&self, substate_id: &tari_engine_types::substate::SubstateId) -> Result<(u32, bool), tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_max_version_for_substate(substate_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_max_version_for_substate(substate_id),
        }
    }
    
    fn substates_any_exist<I, S>(&self, substates: I) -> Result<bool, tari_dan_storage::StorageError>
    where
        I: IntoIterator<Item = S>,
        S: std::borrow::Borrow<tari_dan_common_types::VersionedSubstateId> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_any_exist(substates),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_any_exist(substates),
        }
    }
    
    fn substates_exists_for_transaction(&self, transaction_id: &tari_transaction::TransactionId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_exists_for_transaction(transaction_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_exists_for_transaction(transaction_id),
        }
    }
    
    fn substates_get_n_after(&self, n: usize, after: &tari_dan_common_types::SubstateAddress) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_n_after(n, after),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_n_after(n, after),
        }
    }
    
    fn substates_get_many_within_range(
        &self,
        start: &tari_dan_common_types::SubstateAddress,
        end: &tari_dan_common_types::SubstateAddress,
        exclude_shards: &[tari_dan_common_types::SubstateAddress],
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_many_within_range(start, end, exclude_shards),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_many_within_range(start, end, exclude_shards),
        }
    }
    
    fn substates_get_many_by_created_transaction(
        &self,
        tx_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_many_by_created_transaction(tx_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_many_by_created_transaction(tx_id),
        }
    }
    
    fn substates_get_many_by_destroyed_transaction(
        &self,
        tx_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_many_by_destroyed_transaction(tx_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_many_by_destroyed_transaction(tx_id),
        }
    }
    
    fn substates_get_all_for_transaction(
        &self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::SubstateRecord>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substates_get_all_for_transaction(transaction_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substates_get_all_for_transaction(transaction_id),
        }
    }
    
    fn substate_locks_get_locked_substates_for_transaction(
        &self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<Vec<tari_dan_storage::consensus_models::LockedSubstateValue>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substate_locks_get_locked_substates_for_transaction(transaction_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substate_locks_get_locked_substates_for_transaction(transaction_id),
        }
    }
    
    fn substate_locks_get_latest_for_substate(&self, substate_id: &tari_engine_types::substate::SubstateId) -> Result<tari_dan_storage::consensus_models::SubstateLock, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.substate_locks_get_latest_for_substate(substate_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.substate_locks_get_latest_for_substate(substate_id),
        }
    }
    
    fn pending_state_tree_diffs_get_all_up_to_commit_block(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
    ) -> Result<std::collections::HashMap<tari_dan_common_types::shard::Shard, Vec<tari_dan_storage::consensus_models::PendingShardStateTreeDiff>>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.pending_state_tree_diffs_get_all_up_to_commit_block(block_id),
        }
    }
    
    fn state_transitions_get_n_after(
        &self,
        n: usize,
        id: tari_dan_storage::consensus_models::StateTransitionId,
        end_epoch: tari_dan_common_types::Epoch,
    ) -> Result<Vec<tari_dan_storage::consensus_models::StateTransition>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.state_transitions_get_n_after(n, id, end_epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.state_transitions_get_n_after(n, id, end_epoch),
        }
    }
    
    fn state_transitions_get_last_id(&self, shard: tari_dan_common_types::shard::Shard) -> Result<tari_dan_storage::consensus_models::StateTransitionId, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.state_transitions_get_last_id(shard),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.state_transitions_get_last_id(shard),
        }
    }
    
    fn state_tree_nodes_get(&self, shard: tari_dan_common_types::shard::Shard, key: &NodeKey) -> Result<Node<Version>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.state_tree_nodes_get(shard, key),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.state_tree_nodes_get(shard, key),
        }
    }
    
    fn state_tree_versions_get_latest(&self, shard: tari_dan_common_types::shard::Shard) -> Result<Option<Version>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.state_tree_versions_get_latest(shard),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.state_tree_versions_get_latest(shard),
        }
    }
    
    fn epoch_checkpoint_get(&self, epoch: tari_dan_common_types::Epoch) -> Result<tari_dan_storage::consensus_models::EpochCheckpoint, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.epoch_checkpoint_get(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.epoch_checkpoint_get(epoch),
        }
    }
    
    fn foreign_substate_pledges_exists_for_address<T: tari_dan_common_types::ToSubstateAddress>(
        &self,
        transaction_id: &tari_transaction::TransactionId,
        address: T,
    ) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_substate_pledges_exists_for_address(transaction_id, address),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_substate_pledges_exists_for_address(transaction_id, address),
        }
    }
    
    fn foreign_substate_pledges_get_all_by_transaction_id(
        &self,
        transaction_id: &tari_transaction::TransactionId,
    ) -> Result<tari_dan_storage::consensus_models::SubstatePledges, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_substate_pledges_get_all_by_transaction_id(transaction_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_substate_pledges_get_all_by_transaction_id(transaction_id),
        }
    }
    
    fn burnt_utxos_get(&self, commitment: &UnclaimedConfidentialOutputAddress) -> Result<tari_dan_storage::consensus_models::BurntUtxo, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.burnt_utxos_get(commitment),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.burnt_utxos_get(commitment),
        }
    }
    
    fn burnt_utxos_get_all_unproposed(
        &self,
        leaf_block: &tari_dan_storage::consensus_models::BlockId,
        limit: usize,
    ) -> Result<Vec<tari_dan_storage::consensus_models::BurntUtxo>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.burnt_utxos_get_all_unproposed(leaf_block, limit),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.burnt_utxos_get_all_unproposed(leaf_block, limit),
        }
    }
    
    fn burnt_utxos_count(&self) -> Result<u64, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.burnt_utxos_count(),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.burnt_utxos_count(),
        }
    }
    
    fn foreign_parked_blocks_exists(&self, block_id: &tari_dan_storage::consensus_models::BlockId) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.foreign_parked_blocks_exists(block_id),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.foreign_parked_blocks_exists(block_id),
        }
    }
    
    fn validator_epoch_stats_get(
        &self,
        epoch: tari_dan_common_types::Epoch,
        public_key: &tari_common_types::types::PublicKey,
    ) -> Result<tari_dan_storage::consensus_models::ValidatorConsensusStats, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.validator_epoch_stats_get(epoch, public_key),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.validator_epoch_stats_get(epoch, public_key),
        }
    }
    
    fn validator_epoch_stats_get_nodes_to_evict(
        &self,
        block_id: &tari_dan_storage::consensus_models::BlockId,
        threshold: u64,
        limit: u64,
    ) -> Result<Vec<tari_common_types::types::PublicKey>, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.validator_epoch_stats_get_nodes_to_evict(block_id, threshold, limit),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.validator_epoch_stats_get_nodes_to_evict(block_id, threshold, limit),
        }
    }
    
    fn suspended_nodes_is_evicted(&self, block_id: &tari_dan_storage::consensus_models::BlockId, public_key: &tari_common_types::types::PublicKey) -> Result<bool, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.suspended_nodes_is_evicted(block_id, public_key),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.suspended_nodes_is_evicted(block_id, public_key),
        }
    }
    
    fn evicted_nodes_count(&self, epoch: Epoch) -> Result<u64, tari_dan_storage::StorageError> {
        match self {
            ValidatorNodeStateStoreReadTransaction::Rocksdb(tx) => tx.evicted_nodes_count(epoch),
            ValidatorNodeStateStoreReadTransaction::Sqlite(tx) => tx.evicted_nodes_count(epoch),
        }
    }
    
    fn transaction_pool_has_pending_state_updates(&self) -> Result<bool, tari_dan_storage::StorageError> {
        todo!()
    }

}