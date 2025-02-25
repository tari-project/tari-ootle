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

use rocksdb::{Transaction, TransactionDB};
use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight, SubstateAddress, SubstateRequirement};
use tari_dan_storage::consensus_models::{BlockId, QcId, SubstateDestroyed, SubstateRecord};
use tari_engine_types::substate::{SubstateId, SubstateValue};
use tari_transaction::TransactionId;

use crate::error::RocksDbStorageError;

use super::{super::utils::{bincode_decode, bincode_encode}, model::{ModelColumnFamily, RocksdbModel}};

// We need to reimplement the "SubstateRecord" struct because of a incompatiblity between bincode and ciborium Value,
// which we use for the substate state.
// The error is simply an obscure "Serde(AnyNotSupported)", probably due to some serde tag
// Ref: https://github.com/bincode-org/bincode/blob/trunk/src/features/serde/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubstateModelData {
    pub substate_id: SubstateId,
    pub version: u32,
    pub substate_value: Option<Vec<u8>>,
    pub state_hash: FixedHash,
    pub created_by_transaction: TransactionId,
    pub created_justify: QcId,
    pub created_block: BlockId,
    pub created_height: NodeHeight,
    pub created_by_shard: Shard,
    pub created_at_epoch: Epoch,
    pub destroyed: Option<SubstateDestroyed>,
}

impl From<SubstateRecord> for SubstateModelData {
    fn from(rec: SubstateRecord) -> Self {
        SubstateModelData {
            substate_id: rec.substate_id,
            version: rec.version,
            substate_value: rec.substate_value.map(|v| v.to_bytes()),
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

impl TryFrom<SubstateModelData> for SubstateRecord {
    type Error = String;

    fn try_from(model: SubstateModelData) -> Result<Self, Self::Error> {
        let substate_value = match model.substate_value {
            Some(value) => {
                let value = SubstateValue::from_bytes(&value)
                    .map_err(|err| err.to_string())?;
                Some(value)
            },
            None => None,
        };

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

pub struct SubstateModel {}

impl SubstateModel {
    pub fn key_from_address(address: &SubstateAddress) -> String {
        format!("{}_{}", Self::key_prefix(), address.to_string())
    }
}

impl RocksdbModel for SubstateModel {
    type Item = SubstateRecord;

    fn key_prefix() -> &'static str {
        "substates"
    }

    fn key(item: &Self::Item) -> String {
        let address = SubstateAddress::from_substate_id(item.substate_id(), item.version());
        Self::key_from_address(&address)
    }

    // We need to override the default trait implementation to encode as SubstateModelData
    fn encode(value: &Self::Item) -> Result<Vec<u8>, RocksDbStorageError> {
        let value = SubstateModelData::from(value.clone());
        let bytes = bincode_encode(&value)?;
        Ok(bytes)
    }

    // We need to override the default trait implementation to decode as SubstateModelData
    fn decode(bytes: Vec<u8>) -> Result<Self::Item, RocksDbStorageError> {
        let value: SubstateModelData = bincode_decode(bytes)?;
        let value: SubstateRecord = value.try_into()
            .map_err(|e| RocksDbStorageError::GeneralError { message: e })?;
        Ok(value)
    }

    fn column_families() -> Vec<&'static str> {
        vec![VersionColumnFamily::name(), CreatedByTxColumnFamily::name(), DestroyedByTxColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        VersionColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;
        CreatedByTxColumnFamily::put(db.clone(), tx, operation, value, main_key_bytes)?;
        DestroyedByTxColumnFamily::put(db, tx, operation, value, main_key_bytes)?;

        Ok(())
    }

    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        VersionColumnFamily::delete(db.clone(), tx, operation, item)?;
        CreatedByTxColumnFamily::delete(db.clone(), tx, operation, item)?;
        DestroyedByTxColumnFamily::delete(db, tx, operation, item)?;
        
        Ok(())
    }
}

// version

pub struct VersionColumnFamily {}

impl VersionColumnFamily {
    pub const NAME: &str = "substates_version";

    pub fn build_key_from_requirement(req: &SubstateRequirement) -> String {
        let version = req.version()
            .map(|version|
                // hexadecimal endcoding with full 0 padding, so the key preserves ordering
                format!{"{version:#018x}"}
            )
            .unwrap_or_default();

        format!("{}_{}_{}", SubstateModel::key_prefix(), req.substate_id, version)
    }
}

impl ModelColumnFamily for VersionColumnFamily {
    type Item = SubstateRecord;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        let req = SubstateRequirement::new(value.substate_id.clone(), Some(value.version));
        Self::build_key_from_requirement(&req)
    }
}

// created by transaction
pub struct CreatedByTxColumnFamily {}

impl CreatedByTxColumnFamily {
    pub const NAME: &str = "substates_created_by_tx";

    pub fn build_key_by_transaction(tx_id: &TransactionId, address_opt: Option<&SubstateAddress>) -> String {
        key_cf_by_tx(tx_id, address_opt)
    }
}

impl ModelColumnFamily for CreatedByTxColumnFamily {
    type Item = SubstateRecord;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        let address = value.to_substate_address();
        Self::build_key_by_transaction(&value.created_by_transaction, Some(&address))
    }
}

// destroyed by transaction
pub struct DestroyedByTxColumnFamily {}

impl DestroyedByTxColumnFamily {
    pub const NAME: &str = "substates_destroyed_by_tx";

    pub fn build_key_by_transaction(tx_id: &TransactionId, address_opt: Option<&SubstateAddress>) -> String {
        key_cf_by_tx(tx_id, address_opt)
    }
}

impl ModelColumnFamily for DestroyedByTxColumnFamily {
    type Item = SubstateRecord;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        let address = value.to_substate_address();
        Self::build_key_by_transaction(&value.created_by_transaction, Some(&address))
    }
}

fn key_cf_by_tx(tx_id: &TransactionId, address_opt: Option<&SubstateAddress>) -> String {
    let address = address_opt.map(|s| s.to_string()).unwrap_or_default();
    format!("{}_{}_{}", SubstateModel::key_prefix(), tx_id, address)        
}