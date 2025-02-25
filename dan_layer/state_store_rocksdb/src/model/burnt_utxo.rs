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

use std::sync::Arc;

use rocksdb::{Transaction, TransactionDB};
use tari_dan_storage::consensus_models::{BlockId, BurntUtxo};
use tari_engine_types::template_models::UnclaimedConfidentialOutputAddress;

use super::traits::ModelColumnFamily;
use crate::{error::RocksDbStorageError, model::traits::RocksdbModel};

pub struct BurntUtxoModel {}

impl BurntUtxoModel {
    pub fn key_from_commitment(commitment: &UnclaimedConfidentialOutputAddress) -> String {
        format!("{}_{}", Self::key_prefix(), commitment)
    }
}

impl RocksdbModel for BurntUtxoModel {
    type Item = BurntUtxo;

    fn key_prefix() -> &'static str {
        "burntutxos"
    }

    fn key(value: &Self::Item) -> String {
        Self::key_from_commitment(&value.commitment)
    }

    fn column_families() -> Vec<&'static str> {
        vec![ProposedInColumnFamily::name()]
    }

    fn put_in_cfs(
        db: Arc<TransactionDB>,
        tx: &mut Transaction<'_, TransactionDB>,
        operation: &'static str,
        value: &Self::Item,
    ) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        ProposedInColumnFamily::put(db.clone(), tx, operation, value, main_key_bytes)?;

        Ok(())
    }

    fn delete_from_cfs(
        db: Arc<TransactionDB>,
        tx: &Transaction<'_, TransactionDB>,
        operation: &'static str,
        item: &Self::Item,
    ) -> Result<(), RocksDbStorageError> {
        ProposedInColumnFamily::delete(db.clone(), tx, operation, item)?;
        Ok(())
    }
}

pub struct ProposedInColumnFamily {}

impl ProposedInColumnFamily {
    pub const NAME: &str = "burntutxos_proposed_in";
    pub const UNPROPOSED_VALUE: &str = "None";

    pub fn build_key_prefix(block_id: &BlockId) -> String {
        format!("{}_{}_", BurntUtxoModel::key_prefix(), block_id)
    }
}

impl ModelColumnFamily for ProposedInColumnFamily {
    type Item = BurntUtxo;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        let proposed_in = value
            .proposed_in_block
            .map(|b| b.to_string())
            .unwrap_or(Self::UNPROPOSED_VALUE.to_owned());
        format!("{}_{}_{}", BurntUtxoModel::key_prefix(), proposed_in, value.commitment)
    }
}
