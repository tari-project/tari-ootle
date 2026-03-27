//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};

use crate::{configured::RealTimeEpochTicker, store::EpochOracleStore};

#[cfg(feature = "base_layer")]
pub mod base_layer;
pub mod configured;
#[cfg(feature = "base_layer")]
pub mod hybrid;
pub mod store;

pub enum EpochOracle<TStore> {
    #[cfg(feature = "base_layer")]
    BaseLayer(base_layer::BaseLayerOracle<TStore>),
    Configured(configured::ConfiguredEpochOracle<TStore, RealTimeEpochTicker>),
    #[cfg(feature = "base_layer")]
    Hybrid(hybrid::HybridEpochOracle<TStore>),
}

#[cfg(feature = "base_layer")]
impl<TStore> EpochEventOracle for EpochOracle<TStore>
where
    TStore: EpochOracleStore + Send + 'static,
    TStore: base_layer::BaseLayerBlockHeaderStore,
{
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        match self {
            EpochOracle::BaseLayer(base_layer) => base_layer.next_epoch_event().await,
            EpochOracle::Configured(configured) => configured.next_epoch_event().await,
            EpochOracle::Hybrid(hybrid) => hybrid.next_epoch_event().await,
        }
    }
}

#[cfg(not(feature = "base_layer"))]
impl<TStore> EpochEventOracle for EpochOracle<TStore>
where TStore: EpochOracleStore + Send + 'static
{
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        match self {
            EpochOracle::Configured(configured) => configured.next_epoch_event().await,
        }
    }
}
