//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};

use crate::store::EpochOracleStore;

#[cfg(feature = "base_layer")]
pub mod base_layer;
pub mod configured;
#[cfg(feature = "base_layer")]
pub mod hybrid;
pub mod store;

pub enum EpochOracle<TStore> {
    #[cfg(feature = "base_layer")]
    BaseLayer(base_layer::BaseLayerOracle<TStore>),
    Configured(configured::ConfiguredEpochOracle<TStore>),
    #[cfg(feature = "base_layer")]
    Hybrid(hybrid::HybridEpochOracle<TStore>),
}

impl<TStore: EpochOracleStore + Send + 'static> EpochEventOracle for EpochOracle<TStore> {
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        match self {
            #[cfg(feature = "base_layer")]
            EpochOracle::BaseLayer(base_layer) => base_layer.next_epoch_event().await,
            EpochOracle::Configured(configured) => configured.next_epoch_event().await,
            #[cfg(feature = "base_layer")]
            EpochOracle::Hybrid(hybrid) => hybrid.next_epoch_event().await,
        }
    }
}
