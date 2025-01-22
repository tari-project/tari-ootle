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
use tari_dan_common_types::Epoch;
use tari_dan_storage::consensus_models::{BlockId, ForeignProposal, ForeignProposalStatus, QuorumCertificate};
use crate::{error::RocksDbStorageError, model::model::RocksdbModel};

use super::model::ModelColumnFamily;

pub struct ForeignProposalModel {}

impl ForeignProposalModel {
    pub fn key_from_block_id(block_id: &BlockId) -> String {
        format!("{}_{}", Self::key_prefix(), block_id)
    }
}

impl RocksdbModel for ForeignProposalModel {
    type Item = ForeignProposal;

    fn key_prefix() -> &'static str {
        "foreignproposals"
    }

    fn key(value: &Self::Item) -> String {
        Self::key_from_block_id(value.block.id())
    }

    fn column_families() -> Vec<&'static str> {
        vec![EpochStatusColumnFamily::name(), ProposedColumnFamily::name(), UnconfirmedColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        EpochStatusColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;

        if value.proposed_by_block.is_some() {
            ProposedColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;
        }

        if value.status != ForeignProposalStatus::Confirmed {
            UnconfirmedColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;
        }

        Ok(())
    }

    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        EpochStatusColumnFamily::delete(db.clone(), tx, operation, item)?;

        if item.proposed_by_block.is_some() {
            ProposedColumnFamily::delete(db.clone(), tx, operation,  item)?;
        }

        if item.status != ForeignProposalStatus::Confirmed {
            UnconfirmedColumnFamily::delete(db.clone(), tx, operation,  item)?;
        }
        
        Ok(())
    }
}

// CF to query proposals by block epoch and status
pub struct EpochStatusColumnFamily {}

impl EpochStatusColumnFamily {
    pub fn key_prefix_from_epoch(epoch: &Epoch) -> String {
        format!("{}_{}_", ForeignProposalModel::key_prefix(), epoch)
    }

    pub fn key_prefix_from_epoch_and_status(epoch: &Epoch, status: &ForeignProposalStatus) -> String {
        format!("{}_{}_{}_", ForeignProposalModel::key_prefix(), epoch, status)
    }
}

impl ModelColumnFamily for EpochStatusColumnFamily {
    type Item = ForeignProposal;

    fn name() -> &'static str {
        "foreignproposals_epoch"
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}_{}_{}", ForeignProposalModel::key_prefix(), value.block.epoch(), value.status, value.block.id())
    }
}

// CF to query proposals by the block_id they were proposed by
pub struct ProposedColumnFamily {}

impl ProposedColumnFamily {
    pub fn key_prefix_from_proposed_by_block(block_id: &BlockId) -> String {
        format!("{}_{}_", ForeignProposalModel::key_prefix(), block_id)
    }
}

impl ModelColumnFamily for ProposedColumnFamily {
    type Item = ForeignProposal;

    fn name() -> &'static str {
        "foreignproposals_proposed"
    }

    fn build_key(value: &Self::Item) -> String {
        let proposed = value.proposed_by_block
            .map(|b| b.to_string())
            .unwrap_or("None".to_owned());
        format!("{}_{}_{}", ForeignProposalModel::key_prefix(), proposed, value.block.id())
    }
}

// CF to query proposals that are not comfirmed yet
pub struct UnconfirmedColumnFamily {}

impl UnconfirmedColumnFamily {
    pub fn key_prefix_by_epoch(epoch: &Epoch) -> String {
        format!("{}_{}_", ForeignProposalModel::key_prefix(), epoch)
    }
}

impl ModelColumnFamily for UnconfirmedColumnFamily {
    type Item = ForeignProposal;

    fn name() -> &'static str {
        "foreignproposals_unconfirmed"
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}_{}", ForeignProposalModel::key_prefix(), value.block.epoch(), value.block.id())
    }
}