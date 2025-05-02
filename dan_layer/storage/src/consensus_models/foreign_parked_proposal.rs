//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, ops::Deref};

use serde::{Deserialize, Serialize};
use tari_dan_common_types::Epoch;
use tari_transaction::TransactionId;

use crate::{
    consensus_models::{BlockId, BlockPledge, CommandsCommitProof, ForeignProposal},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignParkedProposal {
    block_id: BlockId,
    commit_proof: CommandsCommitProof,
    block_pledge: BlockPledge,
}

impl ForeignParkedProposal {
    pub fn new(commit_proof: CommandsCommitProof, block_pledge: BlockPledge) -> Self {
        let block_id = commit_proof.calculate_block_id();
        Self {
            block_id: block_id.into(),
            commit_proof,
            block_pledge,
        }
    }

    pub fn commit_proof(&self) -> &CommandsCommitProof {
        &self.commit_proof
    }

    pub fn into_proposal(self) -> ForeignProposal {
        ForeignProposal::new(self.commit_proof, self.block_pledge)
    }

    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }

    pub fn epoch(&self) -> Epoch {
        Epoch(self.commit_proof.sidechain_block_commit_proof().header.epoch)
    }

    pub fn block_pledge(&self) -> &BlockPledge {
        &self.block_pledge
    }
}

impl ForeignParkedProposal {
    pub fn save<TTx>(&self, tx: &mut TTx) -> Result<bool, StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        if self.exists(&**tx)? {
            return Ok(false);
        }

        self.insert(tx)?;
        Ok(true)
    }

    pub fn insert<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.foreign_parked_blocks_insert(self)
    }

    pub fn exists<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<bool, StorageError> {
        tx.foreign_parked_blocks_exists(self.block_id())
    }

    pub fn add_missing_transactions<'a, TTx: StateStoreWriteTransaction, I: IntoIterator<Item = &'a TransactionId>>(
        &self,
        tx: &mut TTx,
        transaction_ids: I,
    ) -> Result<(), StorageError> {
        tx.foreign_parked_blocks_insert_missing_transactions(self.block_id(), transaction_ids)
    }

    pub fn remove_by_transaction_id<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        transaction_id: &TransactionId,
    ) -> Result<Vec<Self>, StorageError> {
        tx.foreign_parked_blocks_remove_all_by_transaction(transaction_id)
    }
}

impl Display for ForeignParkedProposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ForeignParkedBlock: {}, Epoch({}), Height({}), prf: {}, block_pledge=[{}]",
            self.commit_proof.calculate_block_id(),
            self.commit_proof.sidechain_block_commit_proof().header.epoch,
            self.commit_proof.sidechain_block_commit_proof().header.height,
            self.commit_proof.sidechain_block_commit_proof().proof_elements.len(),
            self.block_pledge(),
        )
    }
}
