//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::{warn, *};
use tari_dan_common_types::{Epoch, VersionedSubstateId};
use tari_dan_storage::{
    consensus_models::{BurntUtxo, SubstateRecord},
    StateStore,
    StorageError,
};
use tari_engine_types::confidential::UnclaimedConfidentialOutput;
use tari_epoch_manager::traits::EpochUtxoStore;

const LOG_TARGET: &str = "tari::application::utxo_store";

pub struct StateUtxoStore<TStore> {
    state_store: TStore,
}

impl<TStore> StateUtxoStore<TStore> {
    pub fn new(state_store: TStore) -> Self {
        Self { state_store }
    }
}

impl<TStore: StateStore + Send + Sync> EpochUtxoStore for StateUtxoStore<TStore> {
    type Error = StorageError;

    fn add_unclaimed_utxo(&mut self, epoch: Epoch, substate: UnclaimedConfidentialOutput) -> Result<(), Self::Error> {
        let address = substate.to_address();
        info!(
            target: LOG_TARGET,
            "⛓️ Burnt UTXO {address} registered at {epoch}",
        );
        self.state_store.with_write_tx(|tx| {
            if SubstateRecord::exists(&**tx, &VersionedSubstateId::new(address, 0))? {
                warn!(
                    target: LOG_TARGET,
                    "❓️ Burnt UTXO {address} already exists. Ignoring.",
                );
                return Ok(());
            }

            BurntUtxo::new(address, substate, epoch).insert(tx)
        })?;
        Ok(())
    }
}
