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

use tari_dan_storage::consensus_models::TransactionPoolRecord;
use tari_transaction::TransactionId;

use crate::error::RocksDbStorageError;

use super::{model::RocksdbModel, transaction_pool_state_update::TransactionPoolStateUpdateModel};

pub struct TransactionPoolModel {}

impl TransactionPoolModel {
    pub fn key_from_transaction_id(tx_id: &TransactionId) -> String {
        format!("{}_{}", Self::key_prefix(), tx_id)
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
            value.is_global(),
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

impl RocksdbModel for TransactionPoolModel {
    type Item = TransactionPoolRecord;

    fn key_prefix() -> &'static str {
        "transactionpool"
    }

    fn key(item: &Self::Item) -> String {
        Self::key_from_transaction_id(item.transaction_id())
    }

    fn column_families() -> Vec<&'static str> {
        vec![]
    }
}
