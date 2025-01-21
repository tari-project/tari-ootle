//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::sync::Arc;

use rocksdb::{Transaction, TransactionDB};
use serde::{de::DeserializeOwned, Serialize};
use tari_dan_storage::Ordering;

use crate::error::RocksDbStorageError;

use super::super::utils::{bincode_decode, bincode_encode};

pub trait ModelColumnFamily {
    type Item: Serialize;

    fn name() -> &'static str;

    fn build_key(value: &Self::Item) -> String;

    fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item, value: &[u8]) -> Result<(), RocksDbStorageError> {
        let key = Self::build_key(&item);
        let cf = db.cf_handle(&Self::name()).unwrap();
        tx.put_cf(cf, key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(()) 
    }

    fn delete(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        let key = Self::build_key(&item);
        let cf = db.cf_handle(&Self::name()).unwrap();
        tx.delete_cf(cf, key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }
}

pub trait RocksdbModel {
    type Item: Serialize + DeserializeOwned;

    fn key_prefix() -> &'static str;

    fn key(item: &Self::Item) -> String;

    fn column_families() -> Vec<&'static str> {
        // It's up to concrete models to override this method
        // We provide a default implementation to simplify all the models that do not have column families
        vec![]
    }

    fn encode(value: &Self::Item) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode_encode(value)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<Self::Item, RocksDbStorageError> {
        let value = bincode_decode(bytes)?;
        Ok(value)
    }

    fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value);
        let encoded_value = Self::encode(value)?;

        // put the value in the default column family
        tx.put(&key, encoded_value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        // insert the main key in each of the column families
        Self::put_in_cfs(db.clone(), tx, operation, &value)?;

        Ok(())
    }

    fn put_in_cfs(_db: Arc<TransactionDB>, _tx: &mut Transaction<'_, TransactionDB>, _operation: &'static str, _value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // It's up to concrete models to override this method
        // We provide a default implementation to simplify all the models that do not have column families
        Ok(())
    }

    fn key_exists(tx: &Transaction<'_, TransactionDB>, operation: &'static str, key: &str) -> Result<bool, RocksDbStorageError> {
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        Ok(value.is_some())
    }

    fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, key: &str) -> Result<Self::Item, RocksDbStorageError> {
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key: key.to_owned(), operation })?;
        let value = Self::decode(bytes)?;
        Ok(value)
    }

    fn get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf_name: &str, operation: &'static str, key_prefix: &str, ordering: Ordering) -> Result<Option<Self::Item>, RocksDbStorageError> {
        let cf = db.cf_handle(cf_name).unwrap();

        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));
       
        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        // get the main key from the CF
        let mut iterator = tx.iterator_cf_opt(cf, options, iterator_mode);
        let Some(value) = iterator.next() else {
            return Ok(None);
        };
        let (_, key_bytes) = value.map_err(|e| RocksDbStorageError::RocksDbError {
            operation,
            source: e,
        })?;
        let key = String::from_utf8(key_bytes.to_vec()).unwrap();

        // get the value
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let value = Self::decode(bytes)?;
        Ok(Some(value))
    }

    fn multi_get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, cf_name: &str, key_prefix: &str, ordering: Ordering) -> Result<Vec<Self::Item>, RocksDbStorageError> {
        let cf = db.cf_handle(cf_name).unwrap();
        
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));

        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        // get all the keys
        let iterator = tx.iterator_cf_opt(cf,options, iterator_mode);
        let keys: Vec<String> = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            // the value is the key in the default CF
            String::from_utf8(value.to_vec()).unwrap()
        })
        .collect();

        // get all the values
        let mut values = vec![];
        for key in keys {
            let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
            let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
            let value = Self::decode(bytes)?;
            values.push(value);
        }

        Ok(values)
    }

    fn count(tx: &Transaction<'_, TransactionDB>, key_prefix: Option<&str>) -> Result<u64, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let key_prefix = key_prefix.unwrap_or_default();
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));
        let iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        let count = iterator.count() as u64;
        Ok(count)
    }

    fn multi_get(tx: &Transaction<'_, TransactionDB>, key_prefix_opt: Option<&str>, ordering: Ordering) -> Result<Vec<Self::Item>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();

        let default_key_prefix = format!("{}_", Self::key_prefix());
        let key_prefix = key_prefix_opt.unwrap_or(&default_key_prefix);
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));

        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        let iterator = tx.iterator_opt( iterator_mode, options);
        let values = iterator.map(|item| {
            let (_, bytes) = item.unwrap();
            Self::decode(bytes.to_vec()).unwrap()
        })
        .collect();

        Ok(values)
    }

    fn delete(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, key: &str) -> Result<(), RocksDbStorageError> {
        // we need to have the main value to delete CF values later
        let value = Self::get(tx, operation, key)?;

        tx.delete(key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
            operation,
            source: e,
        })?;

        // we also need to delete related CF keys
        Self::delete_from_cfs(db.clone(), tx, operation, &value)?;

        Ok(())
    }

    fn delete_from_cfs(_db: Arc<TransactionDB>, _tx: &Transaction<'_, TransactionDB>, _operation: &'static str, _value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // It's up to concrete models to override this method
        // We provide a default implementation to simplify all the models that do not have column families
        Ok(())
    }
}