//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rocksdb::{AsColumnFamilyRef, DB, DBIteratorWithThreadMode, DBPinnableSlice, Error, IteratorMode, ReadOptions};

use crate::{
    dbs::iterator::DbRawKeyValueIterator,
    traits::{RocksDatabase, RocksReader},
};

pub struct ReadOnlyDb {
    db: DB,
}

impl ReadOnlyDb {
    pub fn new(db: DB) -> Self {
        Self { db }
    }

    pub fn cf_handle(&self, name: &str) -> Option<&rocksdb::ColumnFamily> {
        self.db.cf_handle(name)
    }
}

impl RocksDatabase for ReadOnlyDb {
    fn cf_handle(&self, name: &str) -> Option<&rocksdb::ColumnFamily> {
        self.db.cf_handle(name)
    }
}

impl RocksReader for ReadOnlyDb {
    type Db = DB;

    fn get_pinned_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<Option<DBPinnableSlice<'_>>, Error> {
        self.db.get_pinned_cf(cf, key)
    }

    fn iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self::Db> {
        self.db.iterator_cf(cf_handle, mode)
    }

    fn iterator_cf_opt<'a: 'b, 'b, M, R>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        readopts: ReadOptions,
        mode: IteratorMode,
        mapper: M,
    ) -> DbRawKeyValueIterator<'b, Self::Db, M>
    where
        for<'c> M: FnMut(Result<(&'c [u8], &'c [u8]), Error>) -> R,
    {
        let raw = self.db.raw_iterator_cf_opt(cf_handle, readopts);
        DbRawKeyValueIterator::new(raw, mode, mapper)
    }

    fn multi_get_cf<'a, 'b: 'a, K, I, W>(&'a self, keys: I) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: 'b + AsColumnFamilyRef,
    {
        self.db.multi_get_cf(keys)
    }
}
