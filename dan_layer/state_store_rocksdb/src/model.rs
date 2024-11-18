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

use std::sync::Arc;

use rocksdb::{AsColumnFamilyRef, ColumnFamily, ColumnFamilyDescriptor, ColumnFamilyRef, Transaction, TransactionDB};
use serde::{Deserialize, Serialize};
use tari_dan_common_types::NodeHeight;
use tari_dan_storage::consensus_models::{Block, BlockId, Decision, Evidence, LeaderFee, TransactionPoolRecord, TransactionPoolStage, TransactionPoolStatusUpdate};
use tari_engine_types::confidential::validate_elgamal_verifiable_balance_proof;
use tari_transaction::TransactionId;

use crate::error::RocksDbStorageError;


const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransactionPoolStateUpdateModel {
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub transaction_id: TransactionId,
    pub evidence: Evidence,
    pub transaction_fee: u64,
    pub leader_fee: Option<LeaderFee>,
    pub stage: TransactionPoolStage,
    pub local_decision: Option<Decision>,
    pub remote_decision: Option<Decision>,
    pub is_ready: bool,
    pub is_applied: bool,
}

impl TransactionPoolStateUpdateModel {
    pub const KEY_PREFIX: &str = "transactionpoolpendingupdate_";

    fn key(value: &TransactionPoolStateUpdateModel) -> String {
        // the key format allows us to query all updates by block prefix, ordered by height DESC
        let block_id =  value.block_id.to_string();
        // TODO: is there a cleaner way to implement desc key ordering in RocksDb?
        let block_height_desc = NodeHeight(u64::MAX) - value.block_height;
        let tx_id = value.transaction_id.to_string();
        format!("{}_{}_{}_{}", Self::KEY_PREFIX, block_id, block_height_desc, tx_id)
    }

    fn encode(value: &TransactionPoolStateUpdateModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionPoolStateUpdateModel, RocksDbStorageError> {
        let (value, _): (TransactionPoolStateUpdateModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionPoolStateUpdateModel) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value);
        let value_bytes = Self::encode(value)?;
        tx.put(key, value_bytes)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn multi_get(tx: &Transaction<'_, TransactionDB>, _operation: &'static str, prefix: &str) -> Result<Vec<TransactionPoolStateUpdateModel>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}_{}", Self::KEY_PREFIX, prefix);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        let iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            let value = Self::decode(value.to_vec()).unwrap();
            value
        })
        .collect();
        Ok(values)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransactionPoolPendingUpdateModel {
    pub transaction: TransactionPoolRecord,
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub is_applied: bool,
}

impl TransactionPoolPendingUpdateModel {
    pub const KEY_PREFIX: &str = "transactionpoolpendingupdate_";
    //pub const CF_BLOCK_ID: &str = "transactionpoolpendingupdate_block_id";

    fn key(tx_id: &TransactionId) -> String {
        format!("transactionpoolpendingupdate_{}", tx_id.to_string())
    }

    fn encode(value: &TransactionPoolPendingUpdateModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionPoolPendingUpdateModel, RocksDbStorageError> {
        let (value, _): (TransactionPoolPendingUpdateModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionPoolPendingUpdateModel) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value.transaction.transaction_id());
        let value_bytes = Self::encode(value)?;
        tx.put(key, value_bytes)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        /*
        // blocks_id column family
        let cf = db.cf_handle(Self::CF_BLOCK_ID).unwrap();
        let key_cf = Self::key_cf(&value.block_id.to_string());
        tx.put_cf(cf, key_cf, value.transaction.transaction_id().as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;
         */

        Ok(())
    }

    /*
    pub fn multi_get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf: &str, operation: &'static str, key_value: &str) -> Result<Vec<TransactionPoolPendingUpdateModel>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}{}", Self::KEY_PREFIX, key_value);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        // get all the tx_ids from the column family
        let cf = db.cf_handle(cf).unwrap();
        let iterator = tx.iterator_cf_opt(cf, options, rocksdb::IteratorMode::Start);
        let tx_ids: Vec<TransactionId> = iterator.map(|item: Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            TransactionId::try_from(value.to_vec()).unwrap()
        })
        .collect();
        
        // get all the values
        let mut updates = vec![];
        for tx_id in tx_ids {
            let key = Self::key(&tx_id);
            let value = tx.get(&key)
                .map_err(|e| RocksDbStorageError::RocksDbError {
                    operation,
                    source: e,
                })?;
            let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
            let update = Self::decode(bytes)?;
            updates.push(update);
        }

        Ok(updates)
    }
     */
}

pub(crate) struct TransactionPoolModel {}

impl TransactionPoolModel {
    fn key(tx_id: &TransactionId) -> String {
        format!("transactionpool_{}", tx_id.to_string())
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

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, tx_id: &TransactionId) -> Result<TransactionPoolRecord, RocksDbStorageError> {
        let key = Self::key(tx_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let block = Self::decode(bytes)?;
        Ok(block)
    }

    pub fn get_all(tx: &Transaction<'_, TransactionDB>, operation: &'static str) -> Result<Vec<TransactionPoolRecord>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange("transactionpool_".as_bytes()));
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

    pub fn try_convert(
        value: &TransactionPoolRecord,
        update: Option<TransactionPoolStateUpdateModel>,
    ) -> Result<TransactionPoolRecord, RocksDbStorageError> {
        let mut evidence = value.evidence().clone();
        let mut pending_stage = None;
        let mut local_decision = value.local_decision();
        let mut is_ready = value.is_ready();
        let mut remote_decision = value.remote_decision();
        let mut leader_fee = value.leader_fee().cloned();
        let mut transaction_fee = value.transaction_fee();

        if let Some(update) = update {
            evidence = update.evidence;
            is_ready = update.is_ready;
            pending_stage = Some(update.stage);
            local_decision = update.local_decision;
            remote_decision = update.remote_decision;
            leader_fee = update.leader_fee;
            transaction_fee = update.transaction_fee;
        }

        Ok(TransactionPoolRecord::load(
            *value.transaction_id(),
            evidence,
            transaction_fee as u64,
            leader_fee,
            value.current_stage(),
            pending_stage,
            value.original_decision(),
            local_decision,
            remote_decision,
            is_ready,
        ))
    }
}

pub(crate) struct BlockModel {}

impl BlockModel {
    pub const CF_PARENT_ID: &str = "blocks_parent_id";

    pub fn cfs() -> Vec<&'static str> {
        vec![Self::CF_PARENT_ID]
    }

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

    pub fn key_exists(tx: &Transaction<'_, TransactionDB>, operation: &'static str, block_id: &BlockId) -> Result<bool, RocksDbStorageError> {
        let key = Self::key(block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        Ok(value.is_some())
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

    pub fn get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf: &str, operation: &'static str, block_id: &BlockId) -> Result<Block, RocksDbStorageError> {
        // get the column family value for the actual block id
        // TODO: make this a multi-get?
        let cf = db.cf_handle(cf).unwrap();
        let key = Self::key(block_id);
        let value = tx.get_cf(cf, &key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let block_id = BlockId::try_from(bytes).unwrap();

        // get the block
        let key = Self::key(&block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;

        let block = Self::decode(bytes)?;
        Ok(block)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, block: &Block) -> Result<(), RocksDbStorageError> {
        let key = Self::key(block.id());
        let value = Self::encode(block)?;

        // put the whole block in the default column family
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        // blocks_parent_id column family
        let cf = db.cf_handle(Self::CF_PARENT_ID).unwrap();
        let key = Self::key(block.parent());
        tx.put_cf(cf, key, block.id().as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }
}