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
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight, SubstateAddress, VersionedSubstateId};
use tari_dan_storage::{consensus_models::{Block, BlockId, BlockTransactionExecution, Decision, Evidence, LeaderFee, QcId, SubstateDestroyed, SubstateRecord, TransactionPoolRecord, TransactionPoolStage, TransactionPoolStatusUpdate, TransactionRecord, VersionedSubstateIdLockIntent}, Ordering};
use tari_engine_types::{commit_result::{ExecuteResult, RejectReason}, confidential::validate_elgamal_verifiable_balance_proof, substate::{SubstateId, SubstateValue}};
use tari_transaction::{TransactionId, TransactionSignature, UnsignedTransaction};
use tari_utilities::ByteArray;


use crate::error::RocksDbStorageError;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

// We need to reimplement the "SubstateRecord" struct because of a incompatiblity between bincode and ciborium Value,
// which we use for the substate state.
// The error is simply an obscure "Serde(AnyNotSupported)", probably due to some serde tag
// Ref: https://github.com/bincode-org/bincode/blob/trunk/src/features/serde/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SubstateModel {
    pub substate_id: SubstateId,
    pub version: u32,
    pub substate_value: Vec<u8>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub state_hash: FixedHash,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_by_transaction: TransactionId,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_justify: QcId,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub created_block: BlockId,
    pub created_height: NodeHeight,
    pub created_by_shard: Shard,
    pub created_at_epoch: Epoch,
    pub destroyed: Option<SubstateDestroyed>,
}

impl From<SubstateRecord> for SubstateModel {
    fn from(rec: SubstateRecord) -> Self {
        SubstateModel {
            substate_id: rec.substate_id,
            version: rec.version,
            substate_value: rec.substate_value.to_bytes(),
            state_hash: rec.state_hash,
            created_by_transaction: rec.created_by_transaction,
            created_justify: rec.created_justify,
            created_block: rec.created_block,
            created_height: rec.created_height,
            created_by_shard: rec.created_by_shard,
            created_at_epoch: rec.created_at_epoch,
            destroyed: rec.destroyed,
        }
    }
}

impl TryFrom<SubstateModel> for SubstateRecord {
    type Error = String;

    fn try_from(model: SubstateModel) -> Result<Self, Self::Error> {
        let substate_value = SubstateValue::from_bytes(&model.substate_value)
            .map_err(|err| err.to_string())?;

        Ok(SubstateRecord {
            substate_id: model.substate_id,
            version: model.version,
            substate_value,
            state_hash: model.state_hash,
            created_by_transaction: model.created_by_transaction,
            created_justify: model.created_justify,
            created_block: model.created_block,
            created_height: model.created_height,
            created_by_shard: model.created_by_shard,
            created_at_epoch: model.created_at_epoch,
            destroyed: model.destroyed,
        })
    }
}

impl SubstateModel {
    pub const KEY_PREFIX: &str = "substates";

    pub fn cfs() -> Vec<&'static str> {
        vec![]
    }

    fn key(substate: &SubstateRecord) -> String {
        let address = SubstateAddress::from_substate_id(substate.substate_id(), substate.version());
        Self::key_from_address(&address)
    }

    fn key_from_address(address: &SubstateAddress) -> String {
        format!("{}_{}", Self::KEY_PREFIX, address.to_string())
    }

    fn encode(value: &SubstateRecord) -> Result<Vec<u8>, RocksDbStorageError> {
        let value = Self::from(value.clone());
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<SubstateRecord, RocksDbStorageError> {
        let (value, _): (SubstateModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        let value: SubstateRecord = value.try_into()
            .map_err(|e| RocksDbStorageError::GeneralError { message: e })?;
        Ok(value)
    }

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, address: &SubstateAddress) -> Result<SubstateRecord, RocksDbStorageError> {
        let key = Self::key_from_address(address);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let substate = Self::decode(bytes)?;
        Ok(substate)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, substate: &SubstateRecord) -> Result<(), RocksDbStorageError> {
        let key = Self::key(substate);
        let value = Self::encode(substate)?;

        // put the value in the default column family
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;   

        Ok(())
    }

}