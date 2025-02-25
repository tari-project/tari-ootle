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
use tari_dan_storage::consensus_models::{BlockId, QcId, QuorumCertificate};

use super::traits::ModelColumnFamily;
use crate::{error::RocksDbStorageError, model::traits::RocksdbModel};

pub struct QuorumCertificateModel {}

impl QuorumCertificateModel {
    pub fn key_from_qc_id(qc_id: &QcId) -> String {
        format!("{}_{}", Self::key_prefix(), qc_id)
    }
}

impl RocksdbModel for QuorumCertificateModel {
    type Item = QuorumCertificate;

    fn key_prefix() -> &'static str {
        "quorumcertificates"
    }

    fn key(value: &Self::Item) -> String {
        Self::key_from_qc_id(value.id())
    }

    fn column_families() -> Vec<&'static str> {
        vec![BlockColumnFamily::name()]
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

        BlockColumnFamily::put(db.clone(), tx, operation, value, main_key_bytes)?;

        Ok(())
    }

    fn delete_from_cfs(
        db: Arc<TransactionDB>,
        tx: &Transaction<'_, TransactionDB>,
        operation: &'static str,
        item: &Self::Item,
    ) -> Result<(), RocksDbStorageError> {
        BlockColumnFamily::delete(db.clone(), tx, operation, item)?;

        Ok(())
    }
}

pub struct BlockColumnFamily {}

impl BlockColumnFamily {
    pub fn key_from_block_id(block_id: &BlockId) -> String {
        format!("{}_{}", QuorumCertificateModel::key_prefix(), block_id)
    }
}

impl ModelColumnFamily for BlockColumnFamily {
    type Item = QuorumCertificate;

    fn name() -> &'static str {
        "quorumcertificates_block"
    }

    fn build_key(value: &Self::Item) -> String {
        Self::key_from_block_id(value.block_id())
    }
}
