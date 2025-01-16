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

use serde::{Deserialize, Serialize};
use tari_dan_common_types::NodeHeight;
use tari_dan_storage::consensus_models::{BlockId, Decision, Evidence, LeaderFee, TransactionPoolStage};
use tari_transaction::TransactionId;

use super::model::RocksdbModel;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionPoolStateUpdateModelData {
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

pub struct TransactionPoolStateUpdateModel {}

impl TransactionPoolStateUpdateModel {
    pub fn key_prefix_by_block_id(block_id: &BlockId) -> String {
        format!("{}_{}_", Self::key_prefix(), block_id)
    }

    pub fn key_prefix_by_block_id_str(block_id: &str) -> String {
        format!("{}_{}_", Self::key_prefix(), block_id)
    }
}

impl RocksdbModel for TransactionPoolStateUpdateModel {
    type Item = TransactionPoolStateUpdateModelData;

    fn key_prefix() -> &'static str {
        "transactionpoolpendingupdate"
    }

    fn key(value: &Self::Item) -> String {
        // the key format allows us to query all updates by block prefix, ordered by height DESC
        let block_id =  value.block_id.to_string();
        // TODO: is there a cleaner way to implement desc key ordering in RocksDb?
        let block_height_desc = NodeHeight(u64::MAX) - value.block_height;
        let tx_id = value.transaction_id.to_string();
        format!("{}_{}_{}_{}", Self::key_prefix(), block_id, block_height_desc, tx_id)
    }

    fn column_families() -> Vec<&'static str> {
        vec![]
    }
}
