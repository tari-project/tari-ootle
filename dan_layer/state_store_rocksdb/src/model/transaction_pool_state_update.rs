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
    pub local_decision: Decision,
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
