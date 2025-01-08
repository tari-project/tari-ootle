//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    hash::{DefaultHasher, Hasher},
    ops::Deref,
};

use serde::{Deserialize, Serialize};
use tari_common_types::types::FixedHash;
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_common_types::{optional::Optional, Epoch};

use crate::{
    consensus_models::{BlockId, QuorumDecision, ValidatorSignature},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub epoch: Epoch,
    pub block_id: BlockId,
    pub decision: QuorumDecision,
    pub sender_leaf_hash: FixedHash,
    pub signature: ValidatorSignature,
}

impl Vote {
    /// Returns a SIPHASH hash used to uniquely identify this vote
    pub fn get_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(self.epoch.as_u64());
        hasher.write(self.block_id.as_bytes());
        hasher.write_u8(self.decision.as_u8());
        hasher.write(self.sender_leaf_hash.as_slice());
        hasher.write(self.signature.public_key.as_bytes());
        hasher.write(self.signature.signature.get_public_nonce().as_bytes());
        hasher.write(self.signature.signature.get_signature().as_bytes());
        hasher.finish()
    }

    pub fn signature(&self) -> &ValidatorSignature {
        &self.signature
    }
}

impl Vote {
    pub fn exists<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<bool, StorageError> {
        Ok(tx
            .votes_get_by_block_and_sender(&self.block_id, &self.sender_leaf_hash)
            .optional()?
            .is_some())
    }

    pub fn save<TTx>(&self, tx: &mut TTx) -> Result<bool, StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        let exists = self.exists(&**tx)?;
        if !exists {
            self.insert(tx)?;
        }
        Ok(exists)
    }

    pub fn insert<TTx>(&self, tx: &mut TTx) -> Result<(), StorageError>
    where
        TTx: StateStoreWriteTransaction + Deref,
        TTx::Target: StateStoreReadTransaction,
    {
        tx.votes_insert(self)
    }

    pub fn count_for_block<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
    ) -> Result<usize, StorageError> {
        tx.votes_count_for_block(block_id).map(|v| v as usize)
    }

    pub fn get_for_block<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
    ) -> Result<Vec<Self>, StorageError> {
        tx.votes_get_for_block(block_id)
    }

    pub fn delete_all<TTx: StateStoreWriteTransaction>(tx: &mut TTx) -> Result<(), StorageError> {
        tx.votes_delete_all()
    }
}
