//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, VecDeque},
    future::{poll_fn, Future},
    pin::Pin,
    task::{Context, Poll},
};

use log::*;
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{displayable::Displayable, Epoch};
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle, ValidatorNodeChange};
use tokio::{time, time::Sleep};

use super::config::Config;
use crate::{
    configured::Validator,
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::dan::epoch_oracles::configured";

pub struct ConfiguredEpochOracle<TStore> {
    config: Config,
    pending_events: VecDeque<EpochEvent>,
    store: TStore,
    sleep: Option<Pin<Box<Sleep>>>,
    queued_validators: HashMap<Epoch, Vec<Validator>>,
    is_initialized: bool,
    is_done: bool,
}

impl<TStore: EpochOracleStore + Send> ConfiguredEpochOracle<TStore> {
    pub fn new(config: Config, store: TStore) -> Self {
        Self {
            config,
            store,
            is_initialized: false,
            pending_events: VecDeque::new(),
            queued_validators: HashMap::new(),
            sleep: None,
            is_done: false,
        }
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        let epoch = self
            .store
            .get(StoreKey::StaticCurrentEpoch.as_key_bytes())?
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

        let is_initialized = self
            .store
            .get::<bool>(StoreKey::StaticIsInitialized.as_key_bytes())?
            .unwrap_or(false);
        if is_initialized {
            return Ok(());
        }

        let size = self.config.initial_epoch.as_u64() as usize + self.config.validators.len() + 1;
        self.pending_events.reserve(size);
        for epoch in 0..=self.config.initial_epoch.as_u64() {
            let epoch = Epoch::from(epoch);
            let registered_vns = self.config.validators.iter().filter(|v| v.registration_epoch == epoch);

            self.pending_events
                .extend(registered_vns.map(|vn| EpochEvent::NewValidatorRegistered {
                    epoch,
                    claim_public_key: vn.claim_key,
                    validator_node_public_key: vn.public_key,
                }));

            let next_epoch = epoch + Epoch(1);
            if let Some(vns) = self.queued_validators.remove(&next_epoch) {
                self.pending_events
                    .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                        epoch,
                        node_changes: vns
                            .into_iter()
                            .map(|vn| ValidatorNodeChange::Add {
                                claim_public_key: vn.claim_key,
                                validator_node_public_key: vn.public_key,
                                activation_epoch: epoch,
                                minimum_value_promise: 0,
                                shard_key: vn.calculate_shard_key(),
                            })
                            .collect(),
                    });
            }

            self.pending_events.push_back(EpochEvent::EpochChanged {
                epoch,
                epoch_hash: calc_static_epoch_hash(epoch),
            });
        }

        self.store.set(StoreKey::StaticIsInitialized.as_key_bytes(), &true)?;
        self.store
            .set(StoreKey::StaticCurrentEpoch.as_key_bytes(), &self.config.initial_epoch)?;
        Ok(())
    }

    fn prepare_next_epoch(&mut self) -> anyhow::Result<()> {
        let epoch = self
            .store
            .get(StoreKey::StaticCurrentEpoch.as_key_bytes())?
            .unwrap_or_else(Epoch::zero);
        let next_epoch = epoch + Epoch(1);
        self.store
            .set(StoreKey::StaticCurrentEpoch.as_key_bytes(), &next_epoch)?;
        let epoch_hash = calc_static_epoch_hash(next_epoch);

        let next_next_epoch = next_epoch + Epoch(1);
        if let Some(vns) = self.queued_validators.get(&next_next_epoch) {
            // Emit these one epoch before a VN becomes active
            self.pending_events
                .extend(vns.iter().map(|vn| EpochEvent::NewValidatorRegistered {
                    epoch: next_epoch,
                    claim_public_key: vn.claim_key,
                    validator_node_public_key: vn.public_key,
                }))
        }

        if let Some(vns) = self.queued_validators.remove(&next_epoch) {
            self.pending_events
                .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                    epoch,
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
        self.pending_events
            .push_back(EpochEvent::DoneForNow { epoch: next_epoch });

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

            // Oracle always notifies the caller when initial scanning has completed.
            self.pending_events.push_back(EpochEvent::DoneForNow {
                epoch: self.config.initial_epoch,
            });
            self.is_initialized = true;
        }

        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(Some(event));
            }

            if let Some(epoch_time) = self.config.epoch_time {
                if self.sleep.is_none() {
                    self.sleep = Some(Box::pin(time::sleep(epoch_time)));
                }
            }

            if let Some(sleep) = self.sleep.as_mut() {
                if sleep.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                if let Err(err) = self.prepare_next_epoch() {
                    self.is_done = true;
                    return Poll::Ready(Some(EpochEvent::error(err)));
                }
            }
            self.sleep = None;
        }
    }
}

impl<TStore: EpochOracleStore + Send> EpochEventOracle for ConfiguredEpochOracle<TStore> {
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        poll_fn(|cx| self.poll(cx)).await
    }
}

pub fn calc_static_epoch_hash(epoch: Epoch) -> FixedHash {
    const U64_SIZE: usize = size_of::<u64>();
    const HASH_SIZE: usize = FixedHash::byte_size();
    let mut epoch_hash = [0u8; HASH_SIZE];
    epoch_hash[HASH_SIZE - U64_SIZE..].copy_from_slice(&epoch.to_be_bytes());
    epoch_hash.into()
}
