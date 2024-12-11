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
use tari_dan_common_types::{NodeHeight, VersionedSubstateId};
use tari_dan_storage::{consensus_models::{Block, BlockId, BlockTransactionExecution, Decision, Evidence, LeaderFee, TransactionPoolRecord, TransactionPoolStage, TransactionPoolStatusUpdate, TransactionRecord, VersionedSubstateIdLockIntent}, Ordering};
use tari_engine_types::{commit_result::{ExecuteResult, RejectReason}, confidential::validate_elgamal_verifiable_balance_proof};
use tari_transaction::{TransactionId, TransactionSignature, UnsignedTransaction};
use tari_utilities::ByteArray;


use crate::error::RocksDbStorageError;

use super::transaction_pool_state_update::TransactionPoolStateUpdateModel;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();


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
            local_decision = Some(update.local_decision);
            remote_decision = update.remote_decision;
            leader_fee = update.leader_fee;
            transaction_fee = update.transaction_fee;
        }

        Ok(TransactionPoolRecord::load(
            *value.transaction_id(),
            evidence,
            transaction_fee as u64,
            leader_fee,
            value.stage(),
            pending_stage,
            value.original_decision(),
            local_decision,
            remote_decision,
            is_ready,
        ))
    }
}
