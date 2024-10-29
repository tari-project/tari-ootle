//  Copyright 2024. The Tari Project
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

use rocksdb::{Transaction, TransactionDB};
use tari_dan_storage::consensus_models::{Block, BlockId, TransactionPoolRecord};
use tari_transaction::TransactionId;

use crate::error::RocksDbStorageError;


const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

pub(crate) struct TransactionPoolModel {}

impl TransactionPoolModel {
    fn key(tx_id: &TransactionId) -> String {
        format!("transaction_pool{}", tx_id.to_string())
    }

    fn encode(value: &TransactionPoolRecord) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionPoolRecord, RocksDbStorageError> {
        let (value, _): (TransactionPoolRecord, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionPoolRecord) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value.transaction_id());
        let value = Self::encode(value)?;
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn get_all(tx: &Transaction<'_, TransactionDB>, operation: &'static str) -> Result<Vec<TransactionPoolRecord>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange("transaction_pool".as_bytes()));
        let iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (key, value) = item.unwrap();
            let value = Self::decode(value.to_vec()).unwrap();
            value
        })
        .collect();
        Ok(values)
    }
}

pub(crate) struct BlockModel {}

impl BlockModel {
    fn key(block_id: &BlockId) -> String {
        format!("blocks_{}", block_id.to_string())
    }

    fn encode(block: &Block) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(block, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<Block, RocksDbStorageError> {
        let (block, _): (Block, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(block)
    }

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, block_id: &BlockId) -> Result<Block, RocksDbStorageError> {
        let key = Self::key(block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let block = Self::decode(bytes)?;
        Ok(block)
    }

    pub fn put(tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, block: &Block) -> Result<(), RocksDbStorageError> {
        let key = Self::key(block.id());
        let value = Self::encode(block)?;
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }
}