//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    marker::PhantomData,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use log::*;
use rocksdb::{
    ColumnFamilyDescriptor,
    DB,
    IteratorMode,
    SingleThreaded,
    SliceTransform,
    TransactionDB,
    TransactionDBOptions,
    TransactionOptions,
    WriteOptions,
};
use serde::{Serialize, de::DeserializeOwned};
use tari_ootle_common_types::NodeAddressable;
use tari_ootle_storage::{StateStore, StorageError};

use crate::{
    column_families::cf_names,
    dbs::read_only::ReadOnlyDb,
    error::RocksDbStorageError,
    info::ColumnFamilyInfo,
    options::DatabaseOptions,
    read_only::ReadOnly,
    read_only_ctx::ReadOnlyContext,
    reader::RocksDbStateStoreReadTransaction,
    snapshot::SnapshotContext,
    traits::{RocksDatabase, RocksReader},
    writer::RocksDbStateStoreWriteTransaction,
};

const LOG_TARGET: &str = "tari::ootle::storage::rocksdb::state_store";

pub fn all_column_families_iter() -> impl Iterator<Item = &'static str> {
    [
        cf_names::BOOKKEEPING,
        cf_names::CHAIN_METADATA,
        cf_names::TRANSACTIONS,
        cf_names::BLOCK,
        cf_names::FOREIGN_PROPOSALS,
        cf_names::CERTIFICATES,
        cf_names::SUBSTATES,
        cf_names::DIAGNOSTICS,
        cf_names::STATE_TREE,
        cf_names::TEMPLATE_METADATA,
    ]
    .into_iter()
}

fn build_default_store_opts() -> rocksdb::Options {
    let mut opts = rocksdb::Options::default();
    // Don't error if the DB exists
    opts.set_error_if_exists(false);
    // Create the DB if it doesn't exist
    opts.create_if_missing(true);
    // Create any missing column families
    opts.create_missing_column_families(true);
    // Schedule background workers instead of using the main worker thread for long-latency operations
    opts.set_avoid_unnecessary_blocking_io(true);
    // All CFs will use a 1-byte prefix extractor
    opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(1));
    // Use a small memtable prefix bloom filter to speed up prefix lookups
    opts.set_memtable_prefix_bloom_ratio(0.05);
    // Better suggested defaults: https://github.com/facebook/rocksdb/wiki/Setup-Options-and-Basic-Tuning
    opts.set_max_background_jobs(6);
    opts.set_bytes_per_sync(1_048_576);
    opts.set_compaction_pri(rocksdb::CompactionPri::MinOverlappingRatio);
    opts.set_level_compaction_dynamic_level_bytes(true);
    let mut bb_opts = rocksdb::BlockBasedOptions::default();
    bb_opts.set_block_size(16 * 1024);
    bb_opts.set_cache_index_and_filter_blocks(true);
    bb_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
    bb_opts.set_format_version(6);
    bb_opts.set_optimize_filters_for_memory(true);

    opts.set_block_based_table_factory(&bb_opts);
    opts
}

pub type RocksDbReadOnlyStateStore<TAddr> = RocksDbStateStore<TAddr, ReadOnlyDb>;
pub struct RocksDbStateStore<TAddr, DB = TransactionDB> {
    db: Arc<DB>,
    options: DatabaseOptions,
    _addr: PhantomData<TAddr>,
}

impl<TAddr> RocksDbStateStore<TAddr, TransactionDB> {
    pub fn open<P: AsRef<Path>>(path: P, options: DatabaseOptions) -> Result<Self, StorageError> {
        let rocks_opts = build_default_store_opts();
        let tx_db_opts = TransactionDBOptions::default();

        let cf_names = all_column_families_iter().map(|name| ColumnFamilyDescriptor::new(name, rocks_opts.clone()));
        let db = TransactionDB::<SingleThreaded>::open_cf_descriptors(&rocks_opts, &tx_db_opts, path, cf_names)
            .map_err(|e| StorageError::ConnectionError {
                reason: e.into_string(),
            })?;
        let db = Self {
            db: Arc::new(db),
            options,
            _addr: PhantomData,
        };

        Ok(db)
    }

    /// Force compact all column families in the database.
    /// This is not typically needed but can be useful for experimentation.
    pub fn compact_all<P: AsRef<Path>>(path: P) -> Result<(), StorageError> {
        let options = build_default_store_opts();
        let cf_names = all_column_families_iter();
        let db = DB::open_cf(&options, path, cf_names).map_err(|e| StorageError::ConnectionError {
            reason: e.into_string(),
        })?;
        for name in all_column_families_iter() {
            let handle = db.cf_handle(name).ok_or_else(|| StorageError::ConnectionError {
                reason: format!("Column family {} not found", name),
            })?;
            db.compact_range_cf(handle, None::<Vec<u8>>, None::<Vec<u8>>);
        }
        Ok(())
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
            options: DatabaseOptions::default(),
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
    type Snapshot<'a>
        = SnapshotContext<'a, TransactionDB>
    where TAddr: 'a;
    type WriteTransaction<'a>
        = RocksDbStateStoreWriteTransaction<'a, Self::Addr>
    where TAddr: 'a;

    fn snapshot(&self) -> Self::Snapshot<'_> {
        let snapshot = self.db.snapshot();
        SnapshotContext::new(&self.db, snapshot)
    }

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, StorageError> {
        let mut opts = TransactionOptions::default();
        let mut write_opts = WriteOptions::new();
        // NOTE: these options are provided because I assume that they have a smaller footprint and
        // (almost) prevent writes. If there are any issues these options, or if the assumptions
        // are incorrect, they can be simply be defaulted.
        opts.set_max_write_batch_size(1);
        write_opts.disable_wal(true);
        let tx = ReadOnly::new(self.db.transaction_opt(&write_opts, &opts));
        Ok(RocksDbStateStoreReadTransaction::new(&self.db, tx))
    }

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, StorageError> {
        let timer = Instant::now();
        let tx = self.db.transaction();
        let tx = RocksDbStateStoreWriteTransaction::new(&self.db, tx, &self.options);
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
            options: self.options.clone(),
        }
    }
}
