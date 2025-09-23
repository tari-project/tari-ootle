//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_ootle_common_types::Epoch;
use tari_ootle_storage::{global, time};

use crate::{error::SqliteStorageError, global::schema::block_headers};

#[derive(Debug, Identifiable, Queryable)]
#[diesel(table_name = block_headers)]
pub struct BlockHeaderModel {
    pub id: i32,
    pub epoch: i64,
    pub height: i64,
    pub block_hash: Vec<u8>,
    pub kernel_merkle_root: Vec<u8>,
    pub validator_node_merkle_root: Vec<u8>,
    pub _created_at: time::PrimitiveDateTime,
}

impl TryFrom<BlockHeaderModel> for global::BlockHeaderModel {
    type Error = SqliteStorageError;

    fn try_from(value: BlockHeaderModel) -> Result<Self, Self::Error> {
        Ok(Self {
            epoch: Epoch(value.epoch as u64),
            height: value.height as u64,
            block_hash: value
                .block_hash
                .try_into()
                .map_err(|e| SqliteStorageError::ConversionError {
                    reason: format!("Block hash invalid: {e}"),
                })?,
            kernel_merkle_root: value.kernel_merkle_root.try_into().map_err(|e| {
                SqliteStorageError::ConversionError {
                    reason: format!("Kernel merkle root invalid: {e}"),
                }
            })?,
            validator_node_merkle_root: value.validator_node_merkle_root.try_into().map_err(|e| {
                SqliteStorageError::ConversionError {
                    reason: format!("Validator node merkle root invalid: {e}"),
                }
            })?,
        })
    }
}
