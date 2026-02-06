//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rocksdb::{
    AsColumnFamilyRef,
    ColumnFamily,
    DBIteratorWithThreadMode,
    DBPinnableSlice,
    Error,
    IteratorMode,
    ReadOptions,
};

use crate::dbs::iterator::DbRawKeyValueIterator;

pub trait RocksDatabase {
    fn cf_handle(&self, name: &str) -> Option<&ColumnFamily>;
}

pub trait RocksReader {
    type Db: rocksdb::DBAccess;
    fn get_pinned_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<Option<DBPinnableSlice<'_>>, Error>;
    fn iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self::Db>;

    fn iterator_cf_opt<'a: 'b, 'b, M, R>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        readopts: ReadOptions,
        mode: IteratorMode,
        mapper: M,
    ) -> DbRawKeyValueIterator<'b, Self::Db, M>
    where
        for<'c> M: FnMut(Result<(&'c [u8], &'c [u8]), Error>) -> R;

    fn multi_get_cf<'a, 'b: 'a, K, I, W>(&'a self, keys: I) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: 'b + AsColumnFamilyRef;
}

pub trait RocksWriter: RocksReader {
    fn put_cf<K: AsRef<[u8]>, V: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        value: V,
    ) -> Result<(), Error>;

    fn delete_cf<K: AsRef<[u8]>>(&self, cf: &impl AsColumnFamilyRef, key: K) -> Result<(), Error>;
}
