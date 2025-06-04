//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rocksdb::{AsColumnFamilyRef, DBIteratorWithThreadMode, DBPinnableSlice, Error, IteratorMode, ReadOptions};

use crate::traits::RocksReader;

#[derive(Debug, Clone)]
pub struct ReadOnly<TX> {
    pub(crate) inner: TX,
}

impl<TX> ReadOnly<TX> {
    pub fn new(inner: TX) -> Self {
        Self { inner }
    }
}

impl<TX: RocksReader> RocksReader for ReadOnly<TX> {
    type Db = TX::Db;

    fn get_pinned_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<Option<DBPinnableSlice>, Error> {
        self.inner.get_pinned_cf(cf, key)
    }

    fn iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self::Db> {
        self.inner.iterator_cf(cf_handle, mode)
    }

    fn iterator_cf_opt<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        readopts: ReadOptions,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self::Db> {
        self.inner.iterator_cf_opt(cf_handle, readopts, mode)
    }

    fn multi_get_cf<'a, 'b: 'a, K, I, W>(&'a self, keys: I) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: 'b + AsColumnFamilyRef,
    {
        self.inner.multi_get_cf(keys)
    }
}
