//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    marker::PhantomData,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use log::log;
use rocksdb::{
    SingleThreaded,
    SnapshotWithThreadMode,
    TransactionDB,
    TransactionDBOptions,
    TransactionOptions,
    WriteOptions,
};
use serde::{de::DeserializeOwned, Serialize};
use tari_dan_common_types::NodeAddressable;
use tari_dan_storage::{StateStore, StorageError};

use crate::{
    model::{
        block,
        block::BlockModel,
        block_diff,
        block_diff::BlockDiffModel,
        block_transaction_execution,
        block_transaction_execution::BlockTransactionExecutionModel,
        bookkeeping,
        burnt_utxo,
        burnt_utxo::BurntUtxoModel,
        chain,
        epoch_checkpoint::EpochCheckpointModel,
        evicted_node::EvictedNodeModel,
        foreign_parked_blocks,
        foreign_parked_blocks::ForeignParkedBlockModel,
        foreign_proposal,
        foreign_proposal::ForeignProposalModel,
        foreign_receive_counter::ForeignReceiveCounterModel,
        foreign_send_counter::ForeignSendCounterModel,
        foreign_substate_pledge::ForeignSubstatePledgeModel,
        lock_conflict,
        lock_conflict::LockConflictModel,
        missing_transactions::MissingTransactionModel,
        parked_block::ParkedBlockModel,
        pending_state_tree_diff::PendingStateTreeDiffModel,
        quorum_certificate::QuorumCertificateModel,
        state_transition,
        state_transition::StateTransitionModel,
        state_tree::{StateTreeModel, StateTreeStaleNodesModelRef},
        state_tree_shard_versions::StateTreeShardVersionModel,
        substate,
        substate::SubstateModel,
        substate_locks,
        substate_locks::SubstateLockModel,
        transaction,
        transaction::TransactionModel,
        transaction_pool::TransactionPoolModel,
        transaction_pool_state_update::TransactionPoolStateUpdateModel,
        validator_node_epoch_stats::ValidatorNodeEpochStatsModel,
        vote::VoteModel,
    },
    reader::RocksDbStateStoreReadTransaction,
    traits::Cf,
    writer::RocksDbStateStoreWriteTransaction,
};

const LOG_TARGET: &str = "tari::dan::storage::rocksdb::state_store";

fn build_default_store_opts() -> rocksdb::Options {
    let mut opts = rocksdb::Options::default();
    opts.set_error_if_exists(false);
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    // TODO: evaluate - might depend on cores?
    opts.set_avoid_unnecessary_blocking_io(true);
    opts
}

pub struct RocksDbStateStore<TAddr> {
    db: Arc<TransactionDB>,
    _addr: PhantomData<TAddr>,
}

impl<TAddr> RocksDbStateStore<TAddr> {
    pub fn connect<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let options = build_default_store_opts();

        let cf_names = Self::all_column_families_iter();
        let db = TransactionDB::<SingleThreaded>::open_cf(&options, &TransactionDBOptions::default(), path, cf_names)
            .map_err(|e| StorageError::ConnectionError {
            reason: e.into_string(),
        })?;

        Ok(Self {
            db: Arc::new(db),
            _addr: PhantomData,
        })
    }

    pub fn snapshot(&self) -> SnapshotWithThreadMode<'_, TransactionDB> {
        self.db.snapshot()
    }

    fn all_column_families_iter() -> impl Iterator<Item = &'static str> {
        [
            bookkeeping::CF_NAME,
            VoteModel::name(),
            chain::PendingChainIndex::name(),
            chain::CommittedParentChildChainIndex::name(),
            chain::PendingParentChildIndex::name(),
            ForeignProposalModel::name(),
            foreign_proposal::ProposedInBlockIndex::name(),
            foreign_proposal::EpochIndex::name(),
            foreign_proposal::UnconfirmedIndex::name(),
            BlockModel::name(),
            block::EpochHeightIndex::name(),
            BlockDiffModel::name(),
            block_diff::SubstateIdIndex::name(),
            QuorumCertificateModel::name(),
            BlockTransactionExecutionModel::name(),
            block_transaction_execution::TransactionIndex::name(),
            TransactionModel::name(),
            transaction::FinalizedAtIndex::name(),
            TransactionPoolModel::name(),
            TransactionPoolStateUpdateModel::name(),
            MissingTransactionModel::name(),
            ParkedBlockModel::name(),
            ForeignParkedBlockModel::name(),
            foreign_parked_blocks::MissingTransactionsModel::name(),
            ForeignSendCounterModel::name(),
            ForeignReceiveCounterModel::name(),
            SubstateLockModel::name(),
            substate_locks::HeadIndex::name(),
            substate_locks::BlockIdIndex::name(),
            substate_locks::SubstateIdIndex::name(),
            SubstateModel::name(),
            substate::HeadIndex::name(),
            substate::TransactionIndex::name(),
            substate::UnprunedDownedValuesIndex::name(),
            StateTransitionModel::name(),
            state_transition::ShardSeqIndex::name(),
            ForeignSubstatePledgeModel::name(),
            PendingStateTreeDiffModel::name(),
            StateTreeModel::name(),
            StateTreeStaleNodesModelRef::name(),
            StateTreeShardVersionModel::name(),
            EpochCheckpointModel::name(),
            BurntUtxoModel::name(),
            burnt_utxo::ProposedInBlockIndex::name(),
            LockConflictModel::name(),
            lock_conflict::ByBlockIdQuery::name(),
            EvictedNodeModel::name(),
            ValidatorNodeEpochStatsModel::name(),
        ]
        .into_iter()
    }
}

// Manually implement the Debug implementation because `RocksDbStateStore` does not implement the Debug trait
impl<TAddr> fmt::Debug for RocksDbStateStore<TAddr> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RocksDbStateStore")
    }
}

impl<TAddr: NodeAddressable + Serialize + DeserializeOwned> StateStore for RocksDbStateStore<TAddr> {
    type Addr = TAddr;
    type ReadTransaction<'a>
        = RocksDbStateStoreReadTransaction<'a, Self::Addr>
    where TAddr: 'a;
    type WriteTransaction<'a>
        = RocksDbStateStoreWriteTransaction<'a, Self::Addr>
    where TAddr: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        let mut opts = TransactionOptions::default();
        let mut write_opts = WriteOptions::new();
        // NOTE: these options are provided because I assume that they have a smaller footprint and
        // (almost) prevent writes. If there are any issues these options, or if the assumptions
        // are incorrect, they can be simply be defaulted.
        opts.set_max_write_batch_size(1);
        write_opts.disable_wal(true);
        let tx = self.db.transaction_opt(&write_opts, &opts);
        Ok(RocksDbStateStoreReadTransaction::new(&self.db, tx))
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        let timer = Instant::now();
        let tx = self.db.transaction();
        let tx = RocksDbStateStoreWriteTransaction::new(&self.db, tx);
        let elapsed = timer.elapsed();
        let level = if elapsed > Duration::from_secs(1) {
            log::Level::Warn
        } else {
            log::Level::Trace
        };
        log!(
            target: LOG_TARGET,
            level,
            "Write transaction obtained in {:?}", elapsed
        );
        Ok(tx)
    }
}

impl<TAddr> Clone for RocksDbStateStore<TAddr> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            _addr: PhantomData,
        }
    }
}
