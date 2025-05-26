//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    any::type_name,
    fmt::{Debug, Display},
};

use rocksdb::{ColumnFamily, IterateBounds, IteratorMode, Transaction, TransactionDB};
use tari_dan_common_types::displayable::Displayable;
use tari_dan_storage::Ordering;

use crate::{
    codecs::{DbCodec, EncodeVec, UnitCodec},
    error::RocksDbStorageError,
    traits::{Cf, QueryCf, RocksReader, RocksWriter},
};

pub struct DbContext<'db> {
    db: &'db TransactionDB,
    tx: &'db Transaction<'db, TransactionDB>,
}

impl<'db> DbContext<'db> {
    pub(crate) fn new(db: &'db TransactionDB, tx: &'db Transaction<'db, TransactionDB>) -> Self {
        Self { db, tx }
    }

    pub fn cf<CF: Cf>(
        &self,
        _cf: CF,
    ) -> Result<CfContext<'db, Transaction<'db, TransactionDB>, CF>, RocksDbStorageError> {
        let handle = self
            .db
            .cf_handle(CF::name())
            .ok_or_else(|| RocksDbStorageError::ColumnFamilyNotFound {
                operation: "create CF context",
                cf: format!("CF={}, cf_name={}", type_name::<CF>(), CF::name()),
            })?;
        CfContext::create(self.tx, handle)
    }
}

pub struct CfContext<'db, DB, CF: Cf> {
    db: &'db DB,
    handle: &'db ColumnFamily,
    key_codec: CF::KeyCodec,
    value_codec: CF::ValueCodec,
}

impl<'db, DB, CF: Cf> CfContext<'db, DB, CF> {
    pub(crate) fn create(db: &'db DB, handle: &'db ColumnFamily) -> Result<Self, RocksDbStorageError> {
        let key_codec = CF::key_codec();
        let value_codec = CF::value_codec();
        Ok(Self {
            db,
            handle,
            key_codec,
            value_codec,
        })
    }
}

impl<CF: Cf, DB: RocksReader> CfContext<'_, DB, CF> {
    pub fn encode_key(&self, key: &CF::Key) -> EncodeVec {
        self.key_codec.encode(key).unwrap_or_else(|e| {
            panic!(
                "database corruption: key encoding failed for CF[{}], key type {}: {}",
                CF::name(),
                type_name::<CF::Key>(),
                e
            )
        })
    }

    pub fn get(&self, key: &CF::Key, operation: &'static str) -> Result<CF::Value, RocksDbStorageError> {
        let key = self.encode_key(key);
        let value = self
            .db
            .get_pinned_cf(self.handle, &key)
            .map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound {
            key: Box::new(key),
            operation,
        })?;
        let value = self.value_codec.decode(&bytes)?;
        Ok(value)
    }

    pub fn get_raw_pinned(
        &self,
        key: &CF::Key,
        operation: &'static str,
    ) -> Result<Option<rocksdb::DBPinnableSlice<'_>>, RocksDbStorageError> {
        let key = self.encode_key(key);
        let value = self
            .db
            .get_pinned_cf(self.handle, &key)
            .map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
        Ok(value)
    }

    pub fn count(&self, operation: &'static str) -> Result<usize, RocksDbStorageError> {
        let iter = self.db.iterator_cf(self.handle, IteratorMode::Start);
        let mut count = 0;
        for result in iter {
            let _unused = result.map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
            count += 1;
        }
        Ok(count)
    }

    pub fn count_prefix(&self, prefix: &CF::Key) -> Result<usize, RocksDbStorageError> {
        let prefix = self.encode_key(prefix);
        let mut opts = rocksdb::ReadOptions::default();
        opts.set_iterate_range(rocksdb::PrefixRange(prefix));
        let iter = self.db.iterator_cf_opt(self.handle, opts, IteratorMode::Start);
        let mut count = 0;
        for result in iter {
            result.map_err(|e| RocksDbStorageError::RocksDbError {
                operation: "count_prefix",
                source: e,
            })?;
            count += 1;
        }
        Ok(count)
    }

    /// Returns true if the key exists in the column family, otherwise false.
    pub fn exists(&self, key: &CF::Key, operation: &'static str) -> Result<bool, RocksDbStorageError> {
        let key = self.encode_key(key);
        let value = self
            .db
            .get_pinned_cf(self.handle, key)
            .map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
        Ok(value.is_some())
    }

    pub fn exists_prefix(&self, prefix: &CF::Key) -> Result<bool, RocksDbStorageError> {
        self.any_exists_within_range(rocksdb::PrefixRange(self.encode_key(prefix)))
    }

    pub fn any_exists_within_range(&self, range: impl IterateBounds) -> Result<bool, RocksDbStorageError> {
        let mut opts = rocksdb::ReadOptions::default();
        opts.set_iterate_range(range);
        let mut iter = self.db.iterator_cf_opt(self.handle, opts, IteratorMode::Start);
        let next = iter.next().transpose().map_err(|e| RocksDbStorageError::RocksDbError {
            operation: "any_exists_within_range",
            source: e,
        })?;
        Ok(next.is_some())
    }

    /// Returns the value for the given keys if the exists. If the key does not exist, it is skipped.
    pub fn multi_get<I, T>(&self, keys: I, operation: &'static str) -> Result<Vec<CF::Value>, RocksDbStorageError>
    where
        I: IntoIterator<Item = T>,
        T: AsRef<CF::Key>,
    {
        let mut keys = keys.into_iter().peekable();
        if keys.peek().is_none() {
            return Ok(Vec::new());
        }

        let keys = keys.map(|k| {
            // We don't support key encoding failing here for mem allocation reasons. Generally key encoding is
            // infallible so we should evaluate whether to change the codec to be infallible. If key encoding on the
            // database level ever fails a crash is reasonable.
            let key = self.key_codec.encode(k.as_ref()).expect("Failed to encode key");
            (self.handle, key)
        });
        let results = self.db.multi_get_cf(keys);

        let mut values = Vec::with_capacity(results.len());

        for result in results {
            let maybe_value = result.map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
            let Some(value) = maybe_value else {
                continue;
            };
            let value = self.value_codec.decode(&value)?;
            values.push(value);
        }

        Ok(values)
    }

    pub fn iterator(
        &self,
        ordering: Ordering,
        operation: &'static str,
    ) -> impl Iterator<Item = Result<(CF::Key, CF::Value), RocksDbStorageError>> + '_ {
        let mode = ordering_to_mode(ordering);
        self.db.iterator_cf(self.handle, mode).map(move |res| {
            res.map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })
                .and_then(|(k, v)| Ok((self.key_codec.decode(&k)?, self.value_codec.decode(&v)?)))
        })
    }

    pub fn key_iterator(
        &self,
        ordering: Ordering,
        operation: &'static str,
    ) -> impl Iterator<Item = Result<CF::Key, RocksDbStorageError>> + '_ {
        let mode = ordering_to_mode(ordering);
        self.db.iterator_cf(self.handle, mode).map(move |res| {
            res.map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })
                .and_then(|(k, _)| self.key_codec.decode(&k))
        })
    }

    pub fn value_iterator(
        &self,
        ordering: Ordering,
        operation: &'static str,
    ) -> impl Iterator<Item = Result<CF::Value, RocksDbStorageError>> + '_ {
        let mode = ordering_to_mode(ordering);
        self.db.iterator_cf(self.handle, mode).map(move |res| {
            res.map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })
                .and_then(|(_, v)| self.value_codec.decode(&v))
        })
    }

    pub fn range_iterator(
        &self,
        ordering: Ordering,
        range: impl IterateBounds,
    ) -> impl Iterator<Item = Result<(CF::Key, CF::Value), RocksDbStorageError>> + '_ {
        let mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };
        let mut opts = rocksdb::ReadOptions::default();
        opts.set_iterate_range(range);
        self.db.iterator_cf_opt(self.handle, opts, mode).map(move |res| {
            res.map_err(|e| RocksDbStorageError::RocksDbError {
                operation: "range_iterator_with_codecs",
                source: e,
            })
            .and_then(|(k, v)| Ok((self.key_codec.decode(&k)?, self.value_codec.decode(&v)?)))
        })
    }

    fn range_iterator_with_codecs<'a, KC, VC, K, V>(
        &'a self,
        ordering: Ordering,
        range: impl IterateBounds,
    ) -> impl Iterator<Item = Result<(K, V), RocksDbStorageError>> + 'a
    where
        KC: DbCodec<K> + Default + 'a,
        VC: DbCodec<V> + Default + 'a,
    {
        let mode = ordering_to_mode(ordering);
        let mut opts = rocksdb::ReadOptions::default();
        opts.set_iterate_range(range);
        let key_codec = KC::default();
        let value_codec = VC::default();
        self.db.iterator_cf_opt(self.handle, opts, mode).map(move |res| {
            res.map_err(|e| RocksDbStorageError::RocksDbError {
                operation: "range_iterator_with_codecs",
                source: e,
            })
            .and_then(|(k, v)| Ok((key_codec.decode(&k)?, value_codec.decode(&v)?)))
        })
    }

    fn range_key_iterator_custom_codec<'a, C, K>(
        &'a self,
        ordering: Ordering,
        range: impl IterateBounds,
    ) -> impl Iterator<Item = Result<K, RocksDbStorageError>> + 'a
    where
        C: DbCodec<K> + Default + 'a,
    {
        let iter = self.range_iterator_with_codecs::<C, UnitCodec, K, ()>(ordering, range);
        iter.map(|res| {
            let (k, _) = res?;
            Ok::<_, RocksDbStorageError>(k)
        })
    }

    pub fn prefix_range_iterator(
        &self,
        ordering: Ordering,
        key: &CF::Key,
    ) -> impl Iterator<Item = Result<(CF::Key, CF::Value), RocksDbStorageError>> + '_ {
        let key = self.encode_key(key);
        self.range_iterator(ordering, rocksdb::PrefixRange(key))
    }

    pub fn prefix_range_iterator_raw_key(
        &self,
        ordering: Ordering,
        key: impl Into<Vec<u8>>,
    ) -> impl Iterator<Item = Result<(CF::Key, CF::Value), RocksDbStorageError>> + '_ {
        self.range_iterator(ordering, rocksdb::PrefixRange(key))
    }

    pub fn prefix_range_key_iterator(
        &self,
        ordering: Ordering,
        key: &CF::Key,
    ) -> impl Iterator<Item = Result<CF::Key, RocksDbStorageError>> + '_ {
        let key = self.encode_key(key);
        let iter = self
            .range_iterator_with_codecs::<CF::KeyCodec, UnitCodec, CF::Key, ()>(ordering, rocksdb::PrefixRange(key));
        iter.map(|res| {
            let (k, _) = res?;
            Ok::<_, RocksDbStorageError>(k)
        })
    }
}

impl<CF, DB> CfContext<'_, DB, CF>
where
    CF: Cf,
    CF::Key: Default,
    DB: RocksReader,
{
    pub fn get_by_default_key(&self, operation: &'static str) -> Result<CF::Value, RocksDbStorageError> {
        self.get(&CF::Key::default(), operation)
    }
}

impl<CF: Cf, DB: RocksWriter> CfContext<'_, DB, CF> {
    pub fn insert(&self, key: &CF::Key, value: &CF::Value, operation: &'static str) -> Result<(), RocksDbStorageError> {
        if self.exists(key, operation)? {
            let key = self.encode_key(key);
            return Err(RocksDbStorageError::ConflictingInsert {
                key: Box::new(key),
                details: "Key already exists".to_string(),
            });
        }

        self.put(key, value, operation)
    }

    pub fn put(&self, key: &CF::Key, value: &CF::Value, operation: &'static str) -> Result<(), RocksDbStorageError> {
        let key = self.key_codec.encode(key)?;
        let encoded_value = self.value_codec.encode(value)?;

        self.db
            .put_cf(self.handle, &key, encoded_value)
            .map_err(|source| RocksDbStorageError::RocksDbError { operation, source })?;

        Ok(())
    }

    /// Checks if the key exists and deletes it. If the key does not exist, an error is returned.
    /// Prefer using `delete` in many cases to save a read operation. This method is useful when the implementer needs
    /// a NotFound error to be returned when the key does not exist.
    /// If the key is known to exist (i.e. just fetched via an iterator), should exist because it is in an index column
    /// family, or the implementer wants idempotency, use `delete`.
    pub fn delete_or_not_found(&self, key: &CF::Key, operation: &'static str) -> Result<(), RocksDbStorageError> {
        if !self.exists(key, operation)? {
            return Err(RocksDbStorageError::NotFound {
                key: Box::new(self.key_codec.encode(key)?),
                operation,
            });
        }
        self.delete(key, operation)?;

        Ok(())
    }

    pub fn delete(&self, key: &CF::Key, operation: &'static str) -> Result<(), RocksDbStorageError> {
        let key = self.key_codec.encode(key)?;
        self.db
            .delete_cf(self.handle, key)
            .map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
        Ok(())
    }
}

pub type QueryCfKv<TQuery> = (
    <<TQuery as QueryCf>::Cf as Cf>::Key,
    <<TQuery as QueryCf>::Cf as Cf>::Value,
);

impl<TQuery: QueryCf, DB: RocksReader> CfContext<'_, DB, TQuery> {
    pub fn query_prefix_range_key_iterator<'a>(
        &'a self,
        ordering: Ordering,
        key: &TQuery::Key,
    ) -> impl Iterator<Item = Result<<TQuery::Cf as Cf>::Key, RocksDbStorageError>> + 'a {
        let key = self.encode_key(key);
        self.range_key_iterator_custom_codec::<<<TQuery as QueryCf>::Cf as Cf>::KeyCodec, <<TQuery as QueryCf>::Cf as Cf>::Key>(
            ordering,
            rocksdb::PrefixRange(key),
        )
    }

    pub fn query_prefix_range_iterator<'a>(
        &'a self,
        ordering: Ordering,
        prefix: &TQuery::Key,
    ) -> impl Iterator<Item = Result<QueryCfKv<TQuery>, RocksDbStorageError>> + 'a {
        let key = self.encode_key(prefix);
        self.range_iterator_with_codecs::<
            <<TQuery as QueryCf>::Cf as Cf>::KeyCodec,
            <<TQuery as QueryCf>::Cf as Cf>::ValueCodec,
            <<TQuery as QueryCf>::Cf as Cf>::Key,
            <<TQuery as QueryCf>::Cf as Cf>::Value,
        >(
            ordering,
            rocksdb::PrefixRange(key),
        )
    }

    pub fn query_prefix_range_value_iterator<'a>(
        &'a self,
        ordering: Ordering,
        prefix: &TQuery::Key,
    ) -> impl Iterator<Item = Result<<<TQuery as QueryCf>::Cf as Cf>::Value, RocksDbStorageError>> + 'a {
        let key = self.encode_key(prefix);
        let iter = self.range_iterator_with_codecs::<UnitCodec, <<TQuery as QueryCf>::Cf as Cf>::ValueCodec, (), <<TQuery as QueryCf>::Cf as Cf>::Value, >( ordering, rocksdb::PrefixRange(key) );
        iter.map(|res| {
            let (_, v) = res?;
            Ok::<_, RocksDbStorageError>(v)
        })
    }

    pub fn query_start_range_key_iterator(
        &self,
        ordering: Ordering,
        start_key: &TQuery::Key,
    ) -> impl Iterator<Item = Result<<TQuery::Cf as Cf>::Key, RocksDbStorageError>> + '_ {
        let key = self.encode_key(start_key);
        let iter = self
            .range_iterator_with_codecs::<<TQuery::Cf as Cf>::KeyCodec, UnitCodec, <TQuery::Cf as Cf>::Key, ()>(
                ordering,
                key..,
            );
        iter.map(|res| {
            let (k, _) = res?;
            Ok::<_, RocksDbStorageError>(k)
        })
    }

    pub fn query_end_range_key_iterator(
        &self,
        ordering: Ordering,
        end_key: &TQuery::Key,
    ) -> impl Iterator<Item = Result<<TQuery::Cf as Cf>::Key, RocksDbStorageError>> + '_ {
        let key = self.encode_key(end_key);
        let iter = self
            .range_iterator_with_codecs::<<TQuery::Cf as Cf>::KeyCodec, UnitCodec, <TQuery::Cf as Cf>::Key, ()>(
                ordering,
                ..key,
            );
        iter.map(|res| {
            let (k, _) = res?;
            Ok::<_, RocksDbStorageError>(k)
        })
    }

    pub fn query_last(&self, operation: &'static str) -> Result<QueryCfKv<TQuery>, RocksDbStorageError> {
        let mut iter = self.db.iterator_cf(self.handle, IteratorMode::End);
        let result = iter.next().ok_or_else(|| RocksDbStorageError::QueryError {
            operation,
            details: format!("No values in TQuery {}", TQuery::name()),
        })?;
        let key_codec = TQuery::make_cf_key_codec();
        let value_codec = TQuery::make_cf_value_codec();
        let (key, value) = result.map_err(|e| RocksDbStorageError::RocksDbError { operation, source: e })?;
        let key = key_codec.decode(&key)?;
        let value = value_codec.decode(&value)?;
        Ok((key, value))
    }
}

impl<CF, DB: RocksReader> CfContext<'_, DB, CF>
where
    CF: Cf,
    CF::Key: Debug,
    CF::Value: Debug,
{
    #[allow(dead_code)]
    pub fn dump_debug(&self) {
        let iter = self.db.iterator_cf(self.handle, IteratorMode::Start);
        let mut count = 0;
        eprintln!("-------------- Dumping CF: {} ------------", CF::name());
        for result in iter {
            let (raw_key, value) = result.unwrap();
            let key = match self.key_codec.decode(&raw_key) {
                Ok(key) => key,
                Err(e) => {
                    eprintln!("Error decoding key: {}: raw: {}", e, raw_key.display());
                    continue;
                },
            };
            let value = self.value_codec.decode(&value).unwrap();
            eprintln!("Key: {:?}, raw: {}, Value: {:?}", key, hex::encode(&raw_key), value);
            count += 1;
        }
        eprintln!("Total: {}", count);
    }
}
impl<CF, DB> CfContext<'_, DB, CF>
where
    DB: RocksReader,
    CF: Cf,
    CF::Key: Display,
    CF::Value: Debug,
{
    #[allow(dead_code)]
    pub fn dump_display(&self) {
        let iter = self.db.iterator_cf(self.handle, IteratorMode::Start);
        let mut count = 0;
        eprintln!("-------------- Dumping CF: {} ------------", CF::name());
        for result in iter {
            let (key, value) = result.unwrap();
            let key = self.key_codec.decode(&key).unwrap();
            let value = self.value_codec.decode(&value).unwrap();
            eprintln!("Key: {}, Value: {:?}", key, value);
            count += 1;
        }
        eprintln!("Total: {}", count);
    }
}

fn ordering_to_mode(ordering: Ordering) -> IteratorMode<'static> {
    match ordering {
        Ordering::Ascending => IteratorMode::Start,
        Ordering::Descending => IteratorMode::End,
    }
}
