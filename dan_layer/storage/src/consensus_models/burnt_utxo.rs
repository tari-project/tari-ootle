//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{hash::Hash, io::Write};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use tari_engine_types::confidential::UnclaimedConfidentialOutput;
use tari_template_lib::models::UnclaimedConfidentialOutputAddress;

use crate::{consensus_models::BlockId, StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurntUtxo {
    pub commitment: UnclaimedConfidentialOutputAddress,
    pub output: UnclaimedConfidentialOutput,
    pub proposed_in_block: Option<BlockId>,
    pub base_layer_block_height: u64,
}

impl BurntUtxo {
    pub fn new(
        commitment: UnclaimedConfidentialOutputAddress,
        output: UnclaimedConfidentialOutput,
        base_layer_block_height: u64,
    ) -> Self {
        Self {
            commitment,
            output,
            proposed_in_block: None,
            base_layer_block_height,
        }
    }

    pub fn to_atom(&self) -> MintConfidentialOutputAtom {
        MintConfidentialOutputAtom {
            commitment: self.commitment,
        }
    }
}

impl BurntUtxo {
    pub fn insert<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.burnt_utxos_insert(self)
    }

    pub fn set_proposed_in_block<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        commitment: &UnclaimedConfidentialOutputAddress,
        proposed_in_block: &BlockId,
    ) -> Result<(), StorageError> {
        tx.burnt_utxos_set_proposed_block(commitment, proposed_in_block)?;
        Ok(())
    }

    pub fn get_all_unproposed<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
        limit: usize,
    ) -> Result<Vec<BurntUtxo>, StorageError> {
        tx.burnt_utxos_get_all_unproposed(block_id, limit)
    }

    pub fn has_unproposed<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<bool, StorageError> {
        Ok(tx.burnt_utxos_count()? > 0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct MintConfidentialOutputAtom {
    pub commitment: UnclaimedConfidentialOutputAddress,
}

impl MintConfidentialOutputAtom {
    pub fn get<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<BurntUtxo, StorageError> {
        tx.burnt_utxos_get(&self.commitment)
    }

    pub fn delete<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.burnt_utxos_delete(&self.commitment)
    }
}

impl BorshSerialize for MintConfidentialOutputAtom {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(&self.commitment.as_object_key().into_array(), writer)
    }
}
