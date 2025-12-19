//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, VecDeque},
    future::poll_fn,
    task::{Context, Poll},
};

use anyhow::Context as _;
use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle, ValidatorNodeChange};
use tari_ootle_common_types::{displayable::Displayable, Epoch};

use super::config::Config;
use crate::{
    configured::{epoch_ticker::EpochTicker, real_time_ticker::RealTimeEpochTicker, EpochTickerData, Validator},
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::configured";

pub struct ConfiguredEpochOracle<TStore, TTicker> {
    config: Config,
    pending_events: VecDeque<EpochEvent>,
    store: TStore,
    ticker: TTicker,
    queued_validators: HashMap<Epoch, Vec<Validator>>,
    is_initialized: bool,
    is_done: bool,
}
impl<TStore: EpochOracleStore + Send> ConfiguredEpochOracle<TStore, RealTimeEpochTicker> {
    pub fn create(config: Config, store: TStore) -> anyhow::Result<Self> {
        let epoch = store
            .get(StoreKey::ConfiguredCurrentEpoch.as_key_bytes())?
            .unwrap_or_else(Epoch::zero);
        let mut ticker = RealTimeEpochTicker::new(config.initial_epoch, config.base_time, epoch);
        if let Some(epoch_time) = config.epoch_time {
            ticker = ticker.with_epoch_time_secs(epoch_time.as_secs().try_into().context("Epoch time cannot be zero")?);
        } else {
            ticker = ticker.disable_ticks();
        }

        Ok(Self {
            config,
            store,
            ticker,
            pending_events: VecDeque::new(),
            queued_validators: HashMap::new(),
            is_initialized: false,
            is_done: false,
        })
    }
}

impl<TStore: EpochOracleStore + Send, TTicker: EpochTicker> ConfiguredEpochOracle<TStore, TTicker> {
    pub fn with_custom_ticker(config: Config, store: TStore, ticker: TTicker) -> Self {
        Self {
            config,
            store,
            ticker,
            pending_events: VecDeque::new(),
            queued_validators: HashMap::new(),
            is_initialized: false,
            is_done: false,
        }
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        let epoch = self
            .store
            .get(StoreKey::ConfiguredCurrentEpoch.as_key_bytes())?
            .unwrap_or_else(Epoch::zero);

        for vn in &self.config.validators {
            if vn.registration_epoch <= epoch {
                continue;
            }

            let vns = self
                .queued_validators
                .entry(vn.registration_epoch + Epoch(1))
                .or_default();
            vns.push(vn.clone());
        }

        info!(
            target: LOG_TARGET,
            "☘️ Starting Configured epoch oracle from {epoch}. Epoch time is {}. {} validator(s) queued",
            self.config.epoch_time.as_ref().map(|d| d.display()).display(),
            self.queued_validators.values().map(|vns| vns.len()).sum::<usize>(),
        );

        Ok(())
    }

    fn prepare_next_epoch(&mut self, epoch_ticker_data: EpochTickerData) -> anyhow::Result<()> {
        let next_epoch = epoch_ticker_data.epoch;
        let epoch_hash = epoch_ticker_data.epoch_hash;
        if next_epoch.is_zero() {
            // If at epoch 0, just emit the epoch changed and done for now events
            self.pending_events.push_back(EpochEvent::EpochChanged {
                epoch: next_epoch,
                epoch_hash,
            });

            return Ok(());
        }

        let done_for_now = epoch_ticker_data.done_for_now;
        let prev_epoch = next_epoch - Epoch(1);
        self.store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &next_epoch)?;

        debug!(
            target: LOG_TARGET,
            "☘️ Preparing next epoch {next_epoch}",
        );
        let next_next_epoch = next_epoch + Epoch(1);
        if let Some(vns) = self.queued_validators.get(&next_next_epoch) {
            debug!(
                target: LOG_TARGET,
                "☘️ {} VNS registered for epoch {next_next_epoch}",
                vns.len()
            );
            // Emit these one epoch before a VN becomes active
            self.pending_events
                .extend(vns.iter().map(|vn| EpochEvent::NewValidatorRegistered {
                    epoch: next_epoch,
                    claim_public_key: vn.claim_key,
                    validator_node_public_key: vn.public_key,
                }))
        }

        if let Some(vns) = self.queued_validators.remove(&next_epoch) {
            debug!(
                target: LOG_TARGET,
                "☘️ {} VNS activated for epoch {next_epoch}",
                vns.len()
            );
            self.pending_events
                .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                    epoch: prev_epoch,
                    node_changes: vns
                        .into_iter()
                        .map(|vn| ValidatorNodeChange::Add {
                            claim_public_key: vn.claim_key,
                            validator_node_public_key: vn.public_key,
                            activation_epoch: next_epoch,
                            minimum_value_promise: 0,
                            shard_key: vn.calculate_shard_key(),
                        })
                        .collect(),
                });
        }

        self.pending_events.push_back(EpochEvent::EpochChanged {
            epoch: next_epoch,
            epoch_hash,
        });

        if done_for_now {
            self.pending_events.push_back(EpochEvent::DoneForNow {
                epoch: next_epoch,
                epoch_hash,
            });
        }

        Ok(())
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<Option<EpochEvent>> {
        if self.is_done {
            return Poll::Ready(None);
        }

        if !self.is_initialized {
            if let Err(err) = self.initialize() {
                self.is_done = true;
                return Poll::Ready(Some(EpochEvent::error(err)));
            }
            self.is_initialized = true;
        }

        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(Some(event));
            }

            match self.ticker.poll_tick(cx) {
                Poll::Ready(Some(data)) => {
                    info!(target: LOG_TARGET, "⏰ Ticker ticked for epoch {}, done_for_now = {}", data.epoch, data.done_for_now);
                    if let Err(err) = self.prepare_next_epoch(data) {
                        self.is_done = true;
                        return Poll::Ready(Some(EpochEvent::error(err)));
                    }
                },
                Poll::Ready(None) => {
                    debug!(target: LOG_TARGET, "Ticker returned None");
                    self.is_done = true;
                    return Poll::Ready(None);
                },
                Poll::Pending => {
                    // Still waiting for the next tick
                    return Poll::Pending;
                },
            }
        }
    }
}

impl<TStore: EpochOracleStore + Send, TTicker: EpochTicker + Send> EpochEventOracle
    for ConfiguredEpochOracle<TStore, TTicker>
{
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        poll_fn(|cx| self.poll(cx)).await
    }
}
