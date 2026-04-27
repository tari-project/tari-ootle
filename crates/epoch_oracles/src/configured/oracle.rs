//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, VecDeque},
    future::poll_fn,
    task::{Context, Poll},
};

use anyhow::Context as _;
use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle, ValidatorNodeChange};
use tari_ootle_common_types::{Epoch, displayable::Displayable};

use super::config::Config;
use crate::{
    configured::{EpochTickerData, Validator, epoch_ticker::EpochTicker, real_time_ticker::RealTimeEpochTicker},
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::configured";

pub struct ConfiguredEpochOracle<TStore, TTicker> {
    config: Config,
    pending_events: VecDeque<EpochEvent>,
    store: TStore,
    ticker: TTicker,
    queued_validators: BTreeMap<Epoch, Vec<Validator>>,
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
            queued_validators: BTreeMap::new(),
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
            queued_validators: BTreeMap::new(),
            is_initialized: false,
            is_done: false,
        }
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        let epoch = self
            .store
            .get(StoreKey::ConfiguredCurrentEpoch.as_key_bytes())?
            .unwrap_or_else(Epoch::zero);

        let mut skipped = 0usize;
        for vn in &self.config.validators {
            let activation_epoch = vn.registration_epoch + Epoch(1);
            // Skip validators whose activation epoch has already been processed in a previous run.
            // Re-queuing them would emit a duplicate ActiveValidatorNodeSetChanged on the next tick,
            // causing duplicate rows in the validator_nodes table. Config changes are only supported
            // when starting from a fresh oracle store.
            if activation_epoch <= epoch {
                skipped += 1;
                continue;
            }

            let vns = self.queued_validators.entry(activation_epoch).or_default();
            vns.push(vn.clone());
        }

        info!(
            target: LOG_TARGET,
            "☘️ Starting Configured epoch oracle from {epoch}. Epoch time is {}. {} validator(s) queued, {skipped} already activated",
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
        let current_epoch = next_epoch - Epoch(1);
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

        // Activate all validators queued at or before next_epoch (handles skipped epochs).
        // split_off returns everything > next_epoch, leaving everything <= next_epoch in place.
        let remaining = self.queued_validators.split_off(&(next_epoch + Epoch(1)));
        let due = std::mem::replace(&mut self.queued_validators, remaining);

        for (epoch, vns) in due {
            debug!(
                target: LOG_TARGET,
                "☘️ {} VNS activated for epoch {epoch} (current: {next_epoch})",
                vns.len()
            );
            self.pending_events
                .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                    epoch: current_epoch,
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

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::Mutex,
        task::{Context, Poll, Waker},
        time::Duration,
    };

    use serde::{Serialize, de::DeserializeOwned};
    use tari_common_types::types::FixedHash;
    use tari_epoch_manager::epoch_event_oracle::EpochEvent;
    use tari_ootle_common_types::{Epoch, ShardGroup};
    use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

    use super::ConfiguredEpochOracle;
    use crate::{
        configured::{Config, EpochTicker, EpochTickerData, Validator},
        store::{EpochOracleStore, StoreKey},
    };

    #[derive(Default)]
    struct TestStore {
        data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    }

    impl EpochOracleStore for TestStore {
        fn get<T: DeserializeOwned>(&self, key: &[u8]) -> anyhow::Result<Option<T>> {
            let data = self.data.lock().unwrap();
            data.get(key).map(|v| Ok(serde_json::from_slice(v)?)).transpose()
        }

        fn set<T: Serialize>(&self, key: &[u8], value: &T) -> anyhow::Result<()> {
            let mut data = self.data.lock().unwrap();
            data.insert(key.to_vec(), serde_json::to_vec(value)?);
            Ok(())
        }
    }

    struct ScriptedTicker {
        data: VecDeque<EpochTickerData>,
    }

    impl ScriptedTicker {
        fn new(data: Vec<EpochTickerData>) -> Self {
            Self { data: data.into() }
        }
    }

    impl EpochTicker for ScriptedTicker {
        fn poll_tick(&mut self, _cx: &mut Context) -> Poll<Option<EpochTickerData>> {
            match self.data.pop_front() {
                Some(d) => Poll::Ready(Some(d)),
                None => Poll::Pending,
            }
        }
    }

    fn mk_validator(public_seed: u8, registration_epoch: Epoch) -> Validator {
        Validator {
            public_key: RistrettoPublicKeyBytes::from_bytes(&[public_seed; 32]).unwrap(),
            claim_key: RistrettoPublicKeyBytes::from_bytes(&[public_seed.wrapping_add(1); 32]).unwrap(),
            shard_group: ShardGroup::new(1, 256),
            registration_epoch,
        }
    }

    fn mk_config(validators: Vec<Validator>) -> Config {
        Config {
            epoch_time: Some(Duration::from_secs(1)),
            initial_epoch: Epoch(0),
            base_time: time::OffsetDateTime::now_utc(),
            validators,
        }
    }

    fn drive_events<T: EpochOracleStore + Send, TT: EpochTicker>(
        oracle: &mut ConfiguredEpochOracle<T, TT>,
    ) -> Vec<EpochEvent> {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut events = vec![];
        while let Poll::Ready(Some(event)) = oracle.poll(&mut cx) {
            events.push(event);
        }
        events
    }

    #[test]
    fn restart_does_not_reactivate_already_activated_validators() {
        let config = mk_config(vec![mk_validator(1, Epoch(10))]);
        let store = TestStore::default();
        // Simulate prior run having processed the validator's activation_epoch (11) already.
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(11))
            .unwrap();

        // Ticker re-emits the last-processed epoch on restart.
        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(11),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker);

        let events = drive_events(&mut oracle);

        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EpochEvent::ActiveValidatorNodeSetChanged { .. })),
            "restart must not re-emit ActiveValidatorNodeSetChanged; got {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EpochEvent::NewValidatorRegistered { .. })),
            "restart must not re-emit NewValidatorRegistered; got {events:?}"
        );
    }

    #[test]
    fn fresh_start_activates_validators_at_activation_epoch() {
        let config = mk_config(vec![mk_validator(1, Epoch(10))]);
        let store = TestStore::default();
        let ticker = ScriptedTicker::new(
            (0..=11)
                .map(|e| EpochTickerData {
                    epoch: Epoch(e),
                    epoch_hash: FixedHash::zero(),
                    done_for_now: e == 11,
                })
                .collect(),
        );
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker);

        let events = drive_events(&mut oracle);

        let activations: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => Some((*epoch, node_changes.len())),
                _ => None,
            })
            .collect();
        assert_eq!(
            activations,
            vec![(Epoch(10), 1)],
            "validator should activate exactly once, announced at epoch 10"
        );

        let announces = events
            .iter()
            .filter(|e| matches!(e, EpochEvent::NewValidatorRegistered { .. }))
            .count();
        assert_eq!(announces, 1, "should announce exactly once");
    }

    #[test]
    fn config_added_after_activation_epoch_is_skipped_on_restart() {
        // User adds a validator with a past registration_epoch and restarts without clearing
        // the oracle store. We skip rather than retroactively activate, since config changes
        // are only supported when starting from a fresh store.
        let config = mk_config(vec![mk_validator(1, Epoch(5))]);
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(20))
            .unwrap();

        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(21),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker);

        let events = drive_events(&mut oracle);

        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EpochEvent::ActiveValidatorNodeSetChanged { .. })),
            "stale-config validator must not be retroactively activated; got {events:?}"
        );
    }
}
