//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};

use crate::{base_layer::BaseLayerOracle, configured::ConfiguredEpochOracle, store::EpochOracleStore};

const LOG_TARGET: &str = "tari::dan::epoch_oracles::hybrid";

pub struct HybridEpochOracle<TStore> {
    configured: ConfiguredEpochOracle<TStore>,
    base_layer: BaseLayerOracle<TStore>,
}

impl<TStore: EpochOracleStore + Send + 'static> HybridEpochOracle<TStore> {
    pub fn new(configured: ConfiguredEpochOracle<TStore>, base_layer: BaseLayerOracle<TStore>) -> Self {
        Self { configured, base_layer }
    }

    fn should_emit_configured(&self, event: &EpochEvent) -> bool {
        match event {
            EpochEvent::Error(_) |
            // Let the configured oracle handle validator nodes
            EpochEvent::ActiveValidatorNodeSetChanged { .. } |
            EpochEvent::NewValidatorRegistered { .. } |
            EpochEvent::NewValidatorNodeExit { .. } |
            EpochEvent::NewCodeTemplateDownload { .. } |
            EpochEvent::NewEvictionProof { .. } => true,
            // These events are handled by the base layer oracle
            EpochEvent::NewConfidentialOutput { .. } |
            EpochEvent::EpochChanged { .. } |
            EpochEvent::DoneForNow { .. } => false,
        }
    }

    fn should_emit_base_layer(&self, event: &EpochEvent) -> bool {
        match event {
            EpochEvent::Error(_) |
            // Let the base layer oracle handle epoch timing and UTXO events
            EpochEvent::NewConfidentialOutput { .. } |
            EpochEvent::EpochChanged { .. } |
            EpochEvent::NewCodeTemplateDownload { .. } |
            EpochEvent::DoneForNow { .. } => true,
            EpochEvent::ActiveValidatorNodeSetChanged { .. } |
            EpochEvent::NewValidatorRegistered { .. } |
            EpochEvent::NewValidatorNodeExit { .. } |
            EpochEvent::NewEvictionProof { .. } => false,
        }
    }
}

impl<TStore: EpochOracleStore + Send + 'static> EpochEventOracle for HybridEpochOracle<TStore> {
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        loop {
            tokio::select! {
                event = self.configured.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "📝 Configured epoch event: {}", event);
                    if self.should_emit_configured(&event) {
                        return Some(event);
                    }
                },
                event = self.base_layer.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "⛓️ Base layer epoch event: {}", event);
                    if self.should_emit_base_layer(&event) {
                        return Some(event);
                    }
                },
            }
        }
    }
}
