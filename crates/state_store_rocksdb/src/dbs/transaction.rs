//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rocksdb::{
    AsColumnFamilyRef,
    DBIteratorWithThreadMode,
    DBPinnableSlice,
    IteratorMode,
    ReadOptions,
    Transaction,
    TransactionDB,
};

use crate::traits::{RocksDatabase, RocksReader, RocksWriter};

impl RocksDatabase for TransactionDB {
    fn cf_handle(&self, name: &str) -> Option<&rocksdb::ColumnFamily> {
        self.cf_handle(name)
    }
}

impl RocksReader for Transaction<'_, TransactionDB> {
    type Db = Self;

    fn get_pinned_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<Option<DBPinnableSlice<'_>>, rocksdb::Error> {
        self.get_pinned_cf(cf, key)
    }

    fn iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self::Db> {
        self.iterator_cf(cf_handle, mode)
    }

    fn iterator_cf_opt<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        readopts: ReadOptions,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self::Db> {
        self.iterator_cf_opt(cf_handle, readopts, mode)
    }

    fn multi_get_cf<'a, 'b: 'a, K, I, W>(&'a self, keys: I) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: 'b + AsColumnFamilyRef,
    {
        let mut read_opts = ReadOptions::default();
        // We always use bulk scans with multi_get, so we disable cache to avoid unnecessary overhead.
        read_opts.fill_cache(false);
        read_opts.set_verify_checksums(false);
        self.multi_get_cf_opt(keys, &read_opts)
    }
}

impl RocksWriter for Transaction<'_, TransactionDB> {
    fn put_cf<K: AsRef<[u8]>, V: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        value: V,
    ) -> Result<(), rocksdb::Error> {
        self.put_cf(cf, key, value)
    }

    fn delete_cf<K: AsRef<[u8]>>(&self, cf: &impl AsColumnFamilyRef, key: K) -> Result<(), rocksdb::Error> {
        self.delete_cf(cf, key)
    }
}
