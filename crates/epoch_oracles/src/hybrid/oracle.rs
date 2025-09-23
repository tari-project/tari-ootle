//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};
use tokio::sync::watch;

use crate::{
    base_layer::BaseLayerOracle,
    configured::{ConfiguredEpochOracle, EpochTickerData},
    hybrid::watch_ticker::WatchEpochTicker,
    store::EpochOracleStore,
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::hybrid";

pub struct HybridEpochOracle<TStore> {
    configured: ConfiguredEpochOracle<TStore, WatchEpochTicker>,
    base_layer: BaseLayerOracle<TStore>,
    trigger: watch::Sender<EpochTickerData>,
    has_initial_sync_completed: bool,
}

impl<TStore: EpochOracleStore + Send + 'static> HybridEpochOracle<TStore> {
    pub fn new(
        configured: ConfiguredEpochOracle<TStore, WatchEpochTicker>,
        base_layer: BaseLayerOracle<TStore>,
        trigger: watch::Sender<EpochTickerData>,
    ) -> Self {
        Self {
            configured,
            base_layer,
            trigger,
            has_initial_sync_completed: false,
        }
    }

    fn should_emit_configured(&self, event: &EpochEvent) -> bool {
        match event {
            EpochEvent::Error(_) |
            // Let the configured oracle handle validator nodes
            EpochEvent::ActiveValidatorNodeSetChanged { .. } |
            EpochEvent::NewValidatorRegistered { .. } |
            EpochEvent::NewValidatorNodeExit { .. } |
            EpochEvent::NewCodeTemplateDownload { .. } |
            EpochEvent::EpochChanged { .. } |
            EpochEvent::NewEvictionProof { .. } |
            EpochEvent::DoneForNow { .. } => true,
            // These events are handled by the base layer oracle
            EpochEvent::NewBlockHeader { .. }  => false
        }
    }

    fn should_emit_base_layer(&self, event: &EpochEvent) -> bool {
        match event {
            EpochEvent::Error(_) |
            // Let the base layer oracle handle epoch timing and UTXO events
            EpochEvent::NewBlockHeader { .. } |
            EpochEvent::NewCodeTemplateDownload { .. } => true,
            EpochEvent::DoneForNow { .. }  |
            EpochEvent::EpochChanged { .. } |
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
                biased;

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
                    match event {
                        // Trigger the watch sender for epoch changes
                        EpochEvent::EpochChanged { epoch, epoch_hash } => {
                            let _ignore = self.trigger.send(EpochTickerData {
                                epoch,
                                epoch_hash,
                                done_for_now: self.has_initial_sync_completed,
                            });
                        },
                        EpochEvent::DoneForNow {epoch, epoch_hash} => {
                            if !self.has_initial_sync_completed {
                                let _ignore = self.trigger.send(EpochTickerData{
                                    epoch,
                                    epoch_hash,
                                    done_for_now: true,
                                });
                                self.has_initial_sync_completed = true;
                            }
                        },
                        // Ignore other events that are not epoch changes
                        _ => {},
                    }
                    if self.should_emit_base_layer(&event) {
                        return Some(event);
                    }
                },
            }
        }
    }
}
