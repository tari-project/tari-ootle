//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};
use tari_ootle_common_types::Epoch;
use tari_sidechain::FixedHash;
use tokio::sync::mpsc;

use crate::{
    base_layer::{BaseLayerBlockHeaderStore, BaseLayerOracle},
    configured::{ConfiguredEpochOracle, EpochTickerData},
    hybrid::mpsc_ticker::MpscEpochTicker,
    store::EpochOracleStore,
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::hybrid";

pub struct HybridEpochOracle<TStore> {
    configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
    base_layer: BaseLayerOracle<TStore>,
    trigger: mpsc::UnboundedSender<EpochTickerData>,
    has_initial_sync_completed: bool,
    last_triggered_epoch: Option<Epoch>,
    /// The last epoch the configured oracle has emitted an `EpochChanged` for.
    last_changed_epoch: Option<Epoch>,
    /// A `DoneForNow` for an epoch the configured oracle has been triggered for but not yet emitted, which follows
    /// that epoch's `EpochChanged`.
    deferred_done_for_now: Option<(Epoch, FixedHash)>,
    ready_event: Option<EpochEvent>,
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static> HybridEpochOracle<TStore> {
    pub fn new(
        configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
        base_layer: BaseLayerOracle<TStore>,
        trigger: mpsc::UnboundedSender<EpochTickerData>,
    ) -> Self {
        Self {
            configured,
            base_layer,
            trigger,
            has_initial_sync_completed: false,
            last_triggered_epoch: None,
            last_changed_epoch: None,
            deferred_done_for_now: None,
            ready_event: None,
        }
    }
}

impl<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static> EpochEventOracle
    for HybridEpochOracle<TStore>
{
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        if let Some(event) = self.ready_event.take() {
            return Some(event);
        }
        loop {
            tokio::select! {
                biased;

                event = self.configured.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "📝 Configured epoch event: {}", event);
                    if let EpochEvent::EpochChanged { epoch, .. } = &event {
                        self.last_changed_epoch = Some(*epoch);
                        if let Some((done_epoch, epoch_hash)) = self.deferred_done_for_now &&
                            done_epoch == *epoch
                        {
                            self.deferred_done_for_now = None;
                            self.ready_event = Some(EpochEvent::DoneForNow { epoch: done_epoch, epoch_hash });
                        }
                    }
                    return Some(event);
                },
                event = self.base_layer.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "⛓️ Base layer epoch event: {}", event);
                    match event {
                        EpochEvent::Error(err) => return Some(EpochEvent::Error(err)),
                        // Trigger the watch sender for epoch changes
                        EpochEvent::EpochChanged { epoch, epoch_hash } => {
                            self.last_triggered_epoch = Some(epoch);
                            let _ignore = self.trigger.send(EpochTickerData {
                                epoch,
                                epoch_hash,
                                done_for_now: self.has_initial_sync_completed,
                            });
                        },
                        EpochEvent::DoneForNow {epoch, epoch_hash} if !self.has_initial_sync_completed => {
                            self.has_initial_sync_completed = true;
                            if self.last_triggered_epoch == Some(epoch) {
                                // The configured oracle may still hold the epoch back, and scanning is only done
                                // once the epoch manager has the epoch.
                                if self.last_changed_epoch == Some(epoch) {
                                    return Some(EpochEvent::DoneForNow { epoch, epoch_hash });
                                }
                                self.deferred_done_for_now = Some((epoch, epoch_hash));
                                continue;
                            }
                            self.last_triggered_epoch = Some(epoch);
                            let _ignore = self.trigger.send(EpochTickerData{
                                epoch,
                                epoch_hash,
                                done_for_now: true,
                            });
                        },
                        // Ignore other events that are not epoch changes
                        _ => {},
                    }
                },
            }
        }
    }

    fn observed_epoch_boundary_hash(&self, epoch: Epoch) -> anyhow::Result<Option<FixedHash>> {
        // Epoch boundaries in hybrid mode are scanned by the base-layer oracle; defer to it.
        self.base_layer.observed_epoch_boundary_hash(epoch)
    }
}
