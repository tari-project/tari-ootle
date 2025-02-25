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

use serde::{Deserialize, Serialize};
use tari_dan_common_types::SubstateAddress;
use tari_dan_storage::consensus_models::SubstatePledge;
use tari_transaction::TransactionId;
use crate::{error::RocksDbStorageError, model::traits::RocksdbModel, utils::{bor_decode, bor_encode}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignSubstatePledgeData {
    pub transaction_id: TransactionId,
    pub substate_address: SubstateAddress,
    pub pledge: SubstatePledge
}

pub struct ForeignSubstatePledgeModel {}

impl ForeignSubstatePledgeModel {
    pub fn key_from_transaction_and_address(transaction_id: &TransactionId, substate_address_opt: Option<&SubstateAddress>) -> String {
        let substate_address = substate_address_opt.map(|a| a.to_string()).unwrap_or_default();
        format!("{}_{}_{}", Self::key_prefix(), transaction_id, substate_address)
    }
}

impl RocksdbModel for ForeignSubstatePledgeModel {
    type Item = ForeignSubstatePledgeData;

    fn key_prefix() -> &'static str {
        "foreignsubstatepledges"
    }

    fn key(value: &Self::Item) -> String {
        Self::key_from_transaction_and_address(&value.transaction_id, Some(&value.substate_address))
    }

    // We need to override the default trait implementations to encode with tari_bor to avoid a bincode conflict with SubstatePledge
    fn encode(value: &Self::Item) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bor_encode(value)?;
        Ok(bytes)
    }
    fn decode(bytes: Vec<u8>) -> Result<Self::Item, RocksDbStorageError> {
        let value: Self::Item = bor_decode(&bytes)?;
        Ok(value)
    }
}