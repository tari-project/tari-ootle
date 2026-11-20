//  Copyright 2022. The Tari Project
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

use tari_ootle_common_types::{Epoch, NodeAddressable, SubstateAddress, VotePower};
use tari_ootle_storage::global::models::ValidatorNode;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

use crate::{
    error::SqliteStorageError,
    global::{schema::*, serialization::deserialize_json},
};

#[derive(Queryable, Identifiable)]
#[diesel(table_name = validator_nodes)]
pub struct DbValidatorNode {
    pub id: i32,
    pub public_key: Vec<u8>,
    pub address: String,
    pub shard_key: Vec<u8>,
    pub start_epoch: i64,
    pub end_epoch: Option<i64>,
    pub fee_claim_public_key: Vec<u8>,
    pub power: i64,
}
impl<TAddr: NodeAddressable> TryFrom<DbValidatorNode> for ValidatorNode<TAddr> {
    type Error = SqliteStorageError;

    fn try_from(vn: DbValidatorNode) -> Result<Self, Self::Error> {
        let vote_power = vn.try_vote_power()?;
        Ok(Self {
            shard_key: SubstateAddress::try_from(vn.shard_key).map_err(|_| {
                SqliteStorageError::MalformedDbData(format!("Invalid shard id in validator node record id={}", vn.id))
            })?,
            address: DbValidatorNode::try_parse_address(&vn.address)?,
            public_key: RistrettoPublicKeyBytes::from_bytes(&vn.public_key).map_err(|_| {
                SqliteStorageError::MalformedDbData(format!("Invalid public key in validator node record id={}", vn.id))
            })?,
            start_epoch: u64::try_from(vn.start_epoch).map(Epoch).map_err(|_| {
                SqliteStorageError::MalformedDbData(format!(
                    "Negative start_epoch {} in validator node record id={}",
                    vn.start_epoch, vn.id
                ))
            })?,
            end_epoch: vn
                .end_epoch
                .map(|e| {
                    u64::try_from(e).map(Epoch).map_err(|_| {
                        SqliteStorageError::MalformedDbData(format!(
                            "Negative end_epoch {e} in validator node record id={}",
                            vn.id
                        ))
                    })
                })
                .transpose()?,
            fee_claim_public_key: RistrettoPublicKeyBytes::from_bytes(&vn.fee_claim_public_key).map_err(|_| {
                SqliteStorageError::MalformedDbData(format!(
                    "Invalid fee claim public key in validator node record id={}",
                    vn.id
                ))
            })?,
            vote_power,
        })
    }
}

impl DbValidatorNode {
    pub fn try_vote_power(&self) -> Result<VotePower, SqliteStorageError> {
        power_from_db(self.power, self.id)
    }

    pub fn try_parse_address<T: serde::de::DeserializeOwned>(address: &str) -> Result<T, SqliteStorageError> {
        deserialize_json(address)
    }
}

pub(crate) fn power_from_db(power: i64, id: i32) -> Result<VotePower, SqliteStorageError> {
    u64::try_from(power).map(VotePower::of).map_err(|_| {
        SqliteStorageError::MalformedDbData(format!("Negative vote power {power} in validator node record id={id}"))
    })
}
