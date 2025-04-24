//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    marker::PhantomData,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use log::{log, warn};
use rocksdb::{
    ColumnFamilyDescriptor,
    IteratorMode,
    SingleThreaded,
    TransactionDB,
    TransactionDBOptions,
    TransactionOptions,
    WriteOptions,
    DB,
};
use serde::{de::DeserializeOwned, Serialize};
use tari_dan_common_types::NodeAddressable;
use tari_dan_storage::{StateStore, StorageError};

use crate::{
    dbs::read_only::ReadOnlyDb,
    error::RocksDbStorageError,
    info::ColumnFamilyInfo,
    models::{
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
        foreign_substate_pledge::ForeignSubstatePledgeModel,
        lock_conflict,
        lock_conflict::LockConflictModel,
        missing_transactions::MissingTransactionModel,
        parked_block::ParkedBlockModel,
        pending_state_tree_diff::PendingStateTreeDiffModel,
        quorum_certificate,
        quorum_certificate::QuorumCertificateModel,
        state_transition,
        state_transition::StateTransitionModel,
        state_tree::{StateTreeModel, StateTreeStaleNodesModel},
        state_tree_shard_versions::StateTreeShardVersionModel,
        substate,
        substate::SubstateModel,
        substate_locks,
        substate_locks::SubstateLockModel,
        transaction::TransactionModel,
        transaction_pool::TransactionPoolModel,
        transaction_pool_state_update::TransactionPoolStateUpdateModel,
        validator_node_epoch_stats::ValidatorNodeEpochStatsModel,
        vote::VoteModel,
    },
    read_only::ReadOnlyContext,
    reader::RocksDbStateStoreReadTransaction,
    snapshot::SnapshotContext,
    traits::{Cf, RocksDatabase, RocksReader},
    writer::RocksDbStateStoreWriteTransaction,
};

const LOG_TARGET: &str = "tari::dan::storage::rocksdb::state_store";

pub fn all_column_families_iter() -> impl Iterator<Item = &'static str> {
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
        quorum_certificate::QuorumCertificateBlockIndex::name(),
        BlockTransactionExecutionModel::name(),
        block_transaction_execution::TransactionIndex::name(),
        TransactionModel::name(),
        TransactionPoolModel::name(),
        TransactionPoolStateUpdateModel::name(),
        MissingTransactionModel::name(),
        ParkedBlockModel::name(),
        ForeignParkedBlockModel::name(),
        foreign_parked_blocks::MissingTransactionsModel::name(),
        SubstateLockModel::name(),
        substate_locks::HeadIndex::name(),
        substate_locks::BlockIdIndex::name(),
        substate_locks::SubstateIdIndex::name(),
        SubstateModel::name(),
        substate::HeadIndex::name(),
        substate::UnprunedDownedValuesIndex::name(),
        StateTransitionModel::name(),
        state_transition::ShardSeqIndex::name(),
        ForeignSubstatePledgeModel::name(),
        PendingStateTreeDiffModel::name(),
        StateTreeModel::name(),
        StateTreeStaleNodesModel::name(),
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

fn build_default_store_opts() -> rocksdb::Options {
    let mut opts = rocksdb::Options::default();
    opts.set_error_if_exists(false);
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    // TODO: evaluate - might depend on cores?
    opts.set_avoid_unnecessary_blocking_io(true);
    opts
}

pub type RocksDbReadOnlyStateStore<TAddr> = RocksDbStateStore<TAddr, ReadOnlyDb>;
pub struct RocksDbStateStore<TAddr, DB = TransactionDB> {
    db: Arc<DB>,
    _addr: PhantomData<TAddr>,
}

impl<TAddr> RocksDbStateStore<TAddr, TransactionDB> {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let options = build_default_store_opts();

        let cf_names = all_column_families_iter();
        let db = TransactionDB::<SingleThreaded>::open_cf(&options, &TransactionDBOptions::default(), path, cf_names)
            .map_err(|e| StorageError::ConnectionError {
            reason: e.into_string(),
        })?;

        Ok(Self {
            db: Arc::new(db),
            _addr: PhantomData,
        })
    }

    pub fn snapshot(&self) -> SnapshotContext<'_, TransactionDB> {
        let snapshot = self.db.snapshot();
        SnapshotContext::new(&self.db, snapshot)
    }
}

impl<TAddr> RocksDbStateStore<TAddr, ReadOnlyDb> {
    pub fn open_read_only<P: AsRef<Path>>(
        path: P,
        secondary_path: P,
    ) -> Result<RocksDbReadOnlyStateStore<TAddr>, StorageError> {
        let options = build_default_store_opts();
        let cf_names = all_column_families_iter().map(|name| ColumnFamilyDescriptor::new(name, options.clone()));
        let db = DB::open_cf_descriptors_as_secondary(&options, path, secondary_path, cf_names).map_err(|e| {
            StorageError::ConnectionError {
                reason: e.into_string(),
            }
        })?;

        Ok(Self {
            db: Arc::new(ReadOnlyDb::new(db)),
            _addr: PhantomData,
        })
    }

    pub fn read_only_context(&self) -> ReadOnlyContext<'_> {
        ReadOnlyContext::new(&self.db)
    }
}

impl<TAddr, DB: RocksDatabase + RocksReader> RocksDbStateStore<TAddr, DB> {
    pub fn column_family_info(&self) -> Result<Vec<ColumnFamilyInfo>, RocksDbStorageError> {
        let mut cf_info = Vec::new();
        for name in all_column_families_iter() {
            let Some(handle) = self.db.cf_handle(name) else {
                warn!(
                    target: LOG_TARGET,
                    "Column family {} not found in database",
                    name
                );
                continue;
            };

            let iter = self.db.iterator_cf(handle, IteratorMode::Start);
            let mut num_entries = 0usize;
            let mut entries_bytes = 0usize;
            for rec in iter {
                let (k, v) = rec.map_err(|e| RocksDbStorageError::RocksDbError {
                    source: e,
                    operation: "column_family_info",
                })?;
                num_entries += 1;
                entries_bytes += k.len() + v.len();
            }
            cf_info.push(ColumnFamilyInfo {
                name: name.to_string(),
                num_entries,
                total_entries_bytes: entries_bytes,
            });
        }

        Ok(cf_info)
    }
}

// Manually implement the Debug implementation because `RocksDbStateStore` does not implement the Debug trait
impl<TAddr, DB> fmt::Debug for RocksDbStateStore<TAddr, DB> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RocksDbStateStore")
    }
}

impl<TAddr: NodeAddressable + Serialize + DeserializeOwned> StateStore for RocksDbStateStore<TAddr, TransactionDB> {
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

impl<TAddr, DB> Clone for RocksDbStateStore<TAddr, DB> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            _addr: PhantomData,
        }
    }
}
