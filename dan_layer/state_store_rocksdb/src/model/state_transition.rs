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

use std::{sync::Arc, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use indexmap::IndexSet;
use rocksdb::{AsColumnFamilyRef, ColumnFamily, ColumnFamilyDescriptor, ColumnFamilyRef, Transaction, TransactionDB};
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{shard::Shard, NodeHeight, SubstateAddress, VersionedSubstateId};
use tari_dan_storage::{consensus_models::{Block, BlockId, BlockTransactionExecution, Decision, Evidence, LeaderFee, StateTransition, StateTransitionId, SubstateRecord, TransactionPoolRecord, TransactionPoolStage, TransactionPoolStatusUpdate, TransactionRecord, VersionedSubstateIdLockIntent}, Ordering};
use tari_engine_types::{commit_result::{ExecuteResult, RejectReason}, confidential::validate_elgamal_verifiable_balance_proof};
use tari_transaction::{TransactionId, TransactionSignature, UnsignedTransaction};
use tari_utilities::ByteArray;


use crate::error::RocksDbStorageError;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionModel {
    pub data: StateTransition,
    pub shard: Shard,
    pub seq: u64
}

impl StateTransitionModel {
    pub const KEY_PREFIX: &str = "statetransitions";
    pub const CF_SHARD_SEQ: &str = "statetransitions_shard_seq";

    pub fn cfs() -> Vec<&'static str> {
        vec![Self::CF_SHARD_SEQ]
    }

    pub fn key(id: &StateTransitionId) -> String {
        format!("{}_{}", Self::KEY_PREFIX, id)
    }

    pub fn key_shard_seq(state: &StateTransitionModel) -> String {
        let seq = state.seq;
        // hexadecimal endcoding with full 0 padding, so the key preserves ordering
        let seq_hex = format!{"{seq:#018x}"};
        format!("{}_{}_{}", Self::KEY_PREFIX, state.shard, seq_hex)
    }

    pub fn key_prefix_shard_seq(shard: &Shard) -> String {
        format!("{}_{}_", Self::KEY_PREFIX, shard)
    }

    fn encode(value: &StateTransitionModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<StateTransitionModel, RocksDbStorageError> {
        let (value, _): (StateTransitionModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn get_by_id(tx: &Transaction<'_, TransactionDB>, operation: &'static str, id: &StateTransitionId) -> Result<StateTransitionModel, RocksDbStorageError> {
        let key = Self::key(id);
        Self::get(tx, operation, &key)
    }

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, key: &str) -> Result<StateTransitionModel, RocksDbStorageError> {
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key: key.to_string(), operation })?;
        let state = Self::decode(bytes)?;
        Ok(state)
    }

    pub fn multi_get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, _operation: &'static str, cf: &str, prefix: &str, ordering: Ordering) -> Result<Vec<String>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}_{}", Self::KEY_PREFIX, prefix);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        let cf = db.cf_handle(cf).unwrap();

        let iterator = tx.iterator_cf_opt(cf,options, iterator_mode);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            // the value is the key in the default CF
            String::from_utf8(value.to_vec()).unwrap()
        })
        .collect();

        Ok(values)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, state: &StateTransitionModel) -> Result<(), RocksDbStorageError> {
        let key = Self::key(&state.data.id);
        let value = Self::encode(state)?;

        // put the value in the default column family
        tx.put(&key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        // key_shard_seq column family
        let cf = db.cf_handle(Self::CF_SHARD_SEQ).unwrap();
        let key_cf = Self::key_shard_seq(state);
        tx.put_cf(cf, key_cf, key.as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;   

        Ok(())
    }

}