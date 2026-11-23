//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::poll_fn,
    iter,
    mem,
    task::{Context, Poll},
};

use anyhow::Context as _;
use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle, ValidatorNodeChange};
use tari_ootle_common_types::{Epoch, SubstateAddress, VotePower, displayable::Displayable};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tokio::sync::mpsc;

use super::config::Config;
use crate::{
    configured::{EpochTickerData, epoch_ticker::EpochTicker, real_time_ticker::RealTimeEpochTicker},
    store::{EpochOracleStore, StoreKey},
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::configured";

/// A validator node registration to announce at a given epoch. Both a validator's initial registration and each of
/// its claim key rotations are emitted as a `ValidatorNodeChange::Add`, which is the only claim-key-bearing change
/// the base layer can express. A rotation reuses the registration's shard key so that changing the claim key does
/// not move the validator within its committee.
#[derive(Debug, Clone)]
struct QueuedActivation {
    public_key: RistrettoPublicKeyBytes,
    claim_key: RistrettoPublicKeyBytes,
    shard_key: SubstateAddress,
    vote_power: VotePower,
    kind: ActivationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivationKind {
    Registration,
    ClaimKeyRotation,
}

pub struct ConfiguredEpochOracle<TStore, TTicker> {
    config: Config,
    pending_events: VecDeque<EpochEvent>,
    unpersisted_epoch: Option<Epoch>,
    store: TStore,
    ticker: TTicker,
    queued_validators: BTreeMap<Epoch, Vec<QueuedActivation>>,
    /// Validators to deactivate, by exit epoch.
    queued_exits: BTreeMap<Epoch, Vec<RistrettoPublicKeyBytes>>,
    /// The last epoch whose activations and exits have been emitted.
    processed_epoch: Epoch,
    config_updates: Option<ConfigUpdates>,
    is_initialized: bool,
    is_done: bool,
}

struct ConfigUpdates {
    receiver: mpsc::UnboundedReceiver<Option<Config>>,
    awaiting_first: bool,
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
            unpersisted_epoch: None,
            queued_validators: BTreeMap::new(),
            queued_exits: BTreeMap::new(),
            processed_epoch: Epoch::zero(),
            config_updates: None,
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
            unpersisted_epoch: None,
            queued_validators: BTreeMap::new(),
            queued_exits: BTreeMap::new(),
            processed_epoch: Epoch::zero(),
            config_updates: None,
            is_initialized: false,
            is_done: false,
        }
    }

    /// Follows the configs sent on `receiver`, adopting each one that agrees with the current config up to the
    /// processed epoch, and holds back epoch changes until the first message arrives. `None` releases them with no
    /// update, for when no config could be fetched. The adopted config is recorded in the store and resumed from on
    /// restart.
    pub fn with_config_updates(mut self, receiver: mpsc::UnboundedReceiver<Option<Config>>) -> Self {
        self.config_updates = Some(ConfigUpdates {
            receiver,
            awaiting_first: true,
        });
        self
    }

    /// Pins the shard key a validator activated with, and rejects a config that no longer derives it.
    ///
    /// The shard key is derived from config and written to the local validator_nodes table when a validator
    /// activates, so editing any field that feeds `Validator::calculate_shard_key` afterwards diverges the network
    /// silently: nodes holding the original row keep the original shard key while a node replaying the edited config
    /// from a fresh store derives a different one, and the two disagree on committee ordering with no epoch boundary
    /// at which to reconcile. Nothing downstream can attribute that, so refuse to start instead.
    fn verify_or_record_shard_key(
        &self,
        public_key: RistrettoPublicKeyBytes,
        shard_key: SubstateAddress,
    ) -> anyhow::Result<()> {
        let mut recorded = self
            .store
            .get::<Vec<(RistrettoPublicKeyBytes, SubstateAddress)>>(
                StoreKey::ConfiguredValidatorShardKeys.as_key_bytes(),
            )?
            .unwrap_or_default();

        match recorded.iter().find(|(pk, _)| *pk == public_key) {
            Some((_, activated_with)) if *activated_with != shard_key => {
                anyhow::bail!(
                    "Validator {public_key} activated with shard key {activated_with} but the current config derives \
                     {shard_key}. A validator's public_key, claim_key, shard_group and registration_epoch are fixed \
                     once it has activated; rotate its claim key with `claim_key_changes` instead of editing \
                     `claim_key`."
                );
            },
            Some(_) => Ok(()),
            None => {
                recorded.push((public_key, shard_key));
                self.store
                    .set(StoreKey::ConfiguredValidatorShardKeys.as_key_bytes(), &recorded)?;
                Ok(())
            },
        }
    }

    /// The exits this node has emitted, as (public key, exit epoch).
    fn applied_exits(&self) -> anyhow::Result<Vec<(RistrettoPublicKeyBytes, Epoch)>> {
        Ok(self
            .store
            .get(StoreKey::ConfiguredAppliedExits.as_key_bytes())?
            .unwrap_or_default())
    }

    /// Rejects a config whose exits up to `epoch` differ from the exits this node has applied.
    ///
    /// A node only applies an exit it reaches with the exit in its config. A node that was given the exit after
    /// passing its epoch keeps the validator in its committees while its peers drop it, and disagrees with them on
    /// the validator set from then on, so refuse to start instead.
    fn verify_applied_exits(&self, epoch: Epoch) -> anyhow::Result<()> {
        let applied = self.applied_exits()?;
        for vn in &self.config.validators {
            let Some(exit_epoch) = vn.exit_epoch.filter(|exit_epoch| *exit_epoch <= epoch) else {
                continue;
            };
            if !applied.contains(&(vn.public_key, exit_epoch)) {
                anyhow::bail!(
                    "Validator {} exits at epoch {exit_epoch}, which this node passed (it is at epoch {epoch}) before \
                     its config held the exit. This node still counts the validator and disagrees with the network on \
                     the validator set. Resync it from a fresh epoch oracle store.",
                    vn.public_key
                );
            }
        }
        for (public_key, exit_epoch) in applied {
            let in_config = self
                .config
                .validators
                .iter()
                .any(|vn| vn.public_key == public_key && vn.exit_epoch == Some(exit_epoch));
            if !in_config {
                anyhow::bail!(
                    "This node applied validator {public_key}'s exit at epoch {exit_epoch}, but the current config \
                     does not hold that exit. An exit is fixed once its epoch is reached; restore it in the config."
                );
            }
        }
        Ok(())
    }

    fn record_applied_exits(&self, exits: &[RistrettoPublicKeyBytes], exit_epoch: Epoch) -> anyhow::Result<()> {
        let mut applied = self.applied_exits()?;
        for public_key in exits {
            if !applied.contains(&(*public_key, exit_epoch)) {
                applied.push((*public_key, exit_epoch));
            }
        }
        self.store
            .set(StoreKey::ConfiguredAppliedExits.as_key_bytes(), &applied)
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        self.config.validate()?;

        let epoch = self
            .store
            .get(StoreKey::ConfiguredCurrentEpoch.as_key_bytes())?
            .unwrap_or_else(Epoch::zero);
        self.processed_epoch = epoch;

        // A node following config updates resumes from the last update it adopted, which matches the validator set
        // it has processed. Updates fetched from then on move it forward.
        if self.config_updates.is_some() &&
            let Some(followed) = self
                .store
                .get::<Config>(StoreKey::ConfiguredFollowedConfig.as_key_bytes())?
        {
            self.config = followed;
        }

        for vn in &self.config.validators {
            // Validators that activated before this node recorded shard keys are pinned to whatever the config
            // derives now. Their rows already exist, so this run's config is the best available record of what they
            // activated with, and it catches any subsequent edit.
            if vn.activation_epoch() <= epoch {
                self.verify_or_record_shard_key(vn.public_key, vn.calculate_shard_key())?;
            }
        }
        self.verify_applied_exits(epoch)?;

        let skipped = self.queue_pending_changes();

        info!(
            target: LOG_TARGET,
            "☘️ Starting Configured epoch oracle from {epoch}. Epoch time is {}. {} validator activation(s) and {} exit(s) queued, {skipped} already applied",
            self.config.epoch_time.as_ref().map(|d| d.display()).display(),
            self.queued_validators.values().map(|vns| vns.len()).sum::<usize>(),
            self.queued_exits.values().map(|vns| vns.len()).sum::<usize>(),
        );

        // Every pending registration is announced on each start, as far ahead of its activation as the config
        // allows, so that a validator scheduled to join can sync its shard group's state before it activates and can
        // resume doing so after a restart. A claim key rotation is not a new registration, so it is not announced.
        self.announce_pending_registrations(|_| true);

        Ok(())
    }

    /// Queues every activation and exit in the config that takes effect after the processed epoch, replacing the
    /// queues, and returns how many were skipped as already applied.
    fn queue_pending_changes(&mut self) -> usize {
        let epoch = self.processed_epoch;
        self.queued_validators.clear();
        self.queued_exits.clear();
        let mut skipped = 0usize;
        for vn in &self.config.validators {
            // A rotation carries the registration's shard key: the new claim key must not reach shard key
            // derivation, or the validator would move within its committee when its claim key changes.
            let shard_key = vn.calculate_shard_key();
            let activations = iter::once((vn.activation_epoch(), vn.claim_key, ActivationKind::Registration)).chain(
                vn.claim_key_changes.iter().map(|change| {
                    (
                        change.effective_epoch,
                        change.claim_key,
                        ActivationKind::ClaimKeyRotation,
                    )
                }),
            );

            for (activation_epoch, claim_key, kind) in activations {
                // Skip activations whose epoch has already been processed in a previous run.
                // Re-queuing them would emit a duplicate ActiveValidatorNodeSetChanged on the next tick,
                // causing duplicate rows in the validator_nodes table. A config update must agree with what was
                // processed; see `Config::check_agrees_up_to`.
                if activation_epoch <= epoch {
                    skipped += 1;
                    continue;
                }

                let vns = self.queued_validators.entry(activation_epoch).or_default();
                vns.push(QueuedActivation {
                    public_key: vn.public_key,
                    claim_key,
                    shard_key,
                    vote_power: vn.vote_power(),
                    kind,
                });
            }

            // An exit at or before the processed epoch was applied by a previous run, like a past activation.
            if let Some(exit_epoch) = vn.exit_epoch {
                if exit_epoch <= epoch {
                    skipped += 1;
                } else {
                    self.queued_exits.entry(exit_epoch).or_default().push(vn.public_key);
                }
            }
        }
        skipped
    }

    /// Announces each queued registration whose public key `filter` accepts.
    fn announce_pending_registrations<F: Fn(&RistrettoPublicKeyBytes) -> bool>(&mut self, filter: F) {
        let epoch = self.processed_epoch;
        let announcements = self
            .queued_validators
            .iter()
            .flat_map(|(activation_epoch, vns)| vns.iter().map(move |vn| (*activation_epoch, vn)))
            .filter(|(_, vn)| vn.kind == ActivationKind::Registration && filter(&vn.public_key))
            .map(|(activation_epoch, vn)| EpochEvent::NewValidatorRegistered {
                epoch,
                claim_public_key: vn.claim_key,
                validator_node_public_key: vn.public_key,
                activation_epoch,
                shard_key: vn.shard_key,
            })
            .collect::<Vec<_>>();
        self.pending_events.extend(announcements);
    }

    /// Adopts `config` if it agrees with the current config on everything up to the processed epoch, and records it
    /// so that a restart resumes from it. A config that does not is ignored with a warning: it came from a peer, and
    /// the current config stays in force.
    fn apply_config_update(&mut self, config: Config) -> anyhow::Result<()> {
        if config == self.config {
            return Ok(());
        }
        if let Err(err) = config.check_agrees_up_to(&self.config, self.processed_epoch) {
            warn!(
                target: LOG_TARGET,
                "☘️ Ignoring epoch oracle config update at epoch {}: {err:#}", self.processed_epoch
            );
            return Ok(());
        }

        self.store
            .set(StoreKey::ConfiguredFollowedConfig.as_key_bytes(), &config)?;
        let prior = mem::replace(&mut self.config, config);
        self.queue_pending_changes();
        info!(
            target: LOG_TARGET,
            "☘️ Adopted epoch oracle config update at epoch {}: {} validator activation(s) and {} exit(s) queued",
            self.processed_epoch,
            self.queued_validators.values().map(|vns| vns.len()).sum::<usize>(),
            self.queued_exits.values().map(|vns| vns.len()).sum::<usize>(),
        );
        self.announce_pending_registrations(|public_key| {
            !prior.validators.iter().any(|vn| vn.public_key == *public_key)
        });
        Ok(())
    }

    fn prepare_next_epoch(&mut self, epoch_ticker_data: EpochTickerData) -> anyhow::Result<()> {
        let next_epoch = epoch_ticker_data.epoch;
        let epoch_hash = epoch_ticker_data.epoch_hash;
        let done_for_now = epoch_ticker_data.done_for_now;
        if next_epoch.is_zero() {
            // If at epoch 0, just emit the epoch changed and done for now events
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

            return Ok(());
        }

        let current_epoch = next_epoch - Epoch(1);

        debug!(
            target: LOG_TARGET,
            "☘️ Preparing next epoch {next_epoch}",
        );
        // Apply all activations and exits queued at or before next_epoch (handles skipped epochs), in epoch order so
        // that a validator activates before it exits.
        let mut due_activations = take_due(&mut self.queued_validators, next_epoch);
        let mut due_exits = take_due(&mut self.queued_exits, next_epoch);
        let due_epochs = due_activations
            .keys()
            .chain(due_exits.keys())
            .copied()
            .collect::<BTreeSet<_>>();

        for epoch in due_epochs {
            if let Some(vns) = due_activations.remove(&epoch) {
                debug!(
                    target: LOG_TARGET,
                    "☘️ {} VNS activated for epoch {epoch} (current: {next_epoch})",
                    vns.len()
                );
                // Pin each shard key as it is committed, so that a later edit to the config that derived it is caught
                // on the next startup even if this node never restarted between the two.
                for vn in vns.iter().filter(|vn| vn.kind == ActivationKind::Registration) {
                    self.verify_or_record_shard_key(vn.public_key, vn.shard_key)?;
                }
                self.pending_events
                    .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                        epoch: current_epoch,
                        node_changes: vns
                            .into_iter()
                            .map(|vn| ValidatorNodeChange::Add {
                                claim_public_key: vn.claim_key,
                                validator_node_public_key: vn.public_key,
                                activation_epoch: epoch,
                                minimum_value_promise: 0,
                                vote_power: vn.vote_power,
                                shard_key: vn.shard_key,
                            })
                            .collect(),
                    });
            }

            if let Some(public_keys) = due_exits.remove(&epoch) {
                debug!(
                    target: LOG_TARGET,
                    "☘️ {} VNS exit at epoch {epoch} (current: {next_epoch})",
                    public_keys.len()
                );
                self.record_applied_exits(&public_keys, epoch)?;
                // The epoch manager ends a registration at the epoch its `Remove` event names, so the event names the
                // exit epoch itself: the validator stays in every committee before it.
                self.pending_events
                    .push_back(EpochEvent::ActiveValidatorNodeSetChanged {
                        epoch,
                        node_changes: public_keys
                            .into_iter()
                            .map(|public_key| ValidatorNodeChange::Remove { public_key })
                            .collect(),
                    });
            }
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

        self.unpersisted_epoch = Some(next_epoch);
        self.processed_epoch = next_epoch;

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

            if let Some(epoch) = self.unpersisted_epoch.take() &&
                let Err(err) = self.store.set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &epoch)
            {
                self.is_done = true;
                return Poll::Ready(Some(EpochEvent::error(err)));
            }

            if let Some(updates) = &mut self.config_updates {
                match updates.receiver.poll_recv(cx) {
                    Poll::Ready(Some(update)) => {
                        updates.awaiting_first = false;
                        if let Some(config) = update &&
                            let Err(err) = self.apply_config_update(config)
                        {
                            self.is_done = true;
                            return Poll::Ready(Some(EpochEvent::error(err)));
                        }
                        continue;
                    },
                    Poll::Ready(None) => {
                        debug!(target: LOG_TARGET, "Epoch oracle config updates closed");
                        self.config_updates = None;
                    },
                    // Epochs wait for the first update so that the changes it schedules are applied in order.
                    Poll::Pending if updates.awaiting_first => return Poll::Pending,
                    Poll::Pending => {},
                }
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

/// Removes and returns every entry of `queue` keyed at or before `epoch`.
fn take_due<T>(queue: &mut BTreeMap<Epoch, T>, epoch: Epoch) -> BTreeMap<Epoch, T> {
    let remaining = match epoch.checked_add(Epoch(1)) {
        Some(after) => queue.split_off(&after),
        None => BTreeMap::new(),
    };
    std::mem::replace(queue, remaining)
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
        num::NonZeroU64,
        sync::Mutex,
        task::{Context, Poll, Waker},
        time::Duration,
    };

    use serde::{Serialize, de::DeserializeOwned};
    use tari_epoch_manager::epoch_event_oracle::{EpochEvent, ValidatorNodeChange};
    use tari_ootle_common_types::{Epoch, ShardGroup, SubstateAddress, VotePower};
    use tari_sidechain::FixedHash;
    use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

    use super::ConfiguredEpochOracle;
    use crate::{
        configured::{ClaimKeyChange, Config, EpochTicker, EpochTickerData, Validator},
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

    fn mk_key(seed: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[seed; 32]).unwrap()
    }

    fn mk_validator(public_seed: u8, registration_epoch: Epoch) -> Validator {
        Validator {
            public_key: mk_key(public_seed),
            claim_key: mk_key(public_seed.wrapping_add(1)),
            shard_group: ShardGroup::new(1, 256),
            registration_epoch,
            claim_key_changes: vec![],
            power: NonZeroU64::MIN,
            exit_epoch: None,
        }
    }

    /// The `Add` changes emitted by each `ActiveValidatorNodeSetChanged`, flattened to
    /// (announcing epoch, claim key, activation epoch, shard key).
    fn added_validators(events: &[EpochEvent]) -> Vec<(Epoch, RistrettoPublicKeyBytes, Epoch, SubstateAddress)> {
        events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => Some((*epoch, node_changes)),
                _ => None,
            })
            .flat_map(|(epoch, node_changes)| {
                node_changes.iter().map(move |change| match change {
                    ValidatorNodeChange::Add {
                        claim_public_key,
                        activation_epoch,
                        shard_key,
                        ..
                    } => (epoch, *claim_public_key, *activation_epoch, *shard_key),
                    ValidatorNodeChange::Remove { .. } => panic!("unexpected Remove change"),
                })
            })
            .collect()
    }

    fn ticker_through(last_epoch: u64) -> ScriptedTicker {
        ScriptedTicker::new(
            (0..=last_epoch)
                .map(|e| EpochTickerData {
                    epoch: Epoch(e),
                    epoch_hash: FixedHash::zero(),
                    done_for_now: e == last_epoch,
                })
                .collect(),
        )
    }

    fn mk_config(validators: Vec<Validator>) -> Config {
        Config {
            epoch_time: Some(Duration::from_secs(1)),
            initial_epoch: Epoch(0),
            base_time: time::OffsetDateTime::UNIX_EPOCH,
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

    /// The (activation epoch, shard key) of each `NewValidatorRegistered` in `events`.
    fn announced_registrations(events: &[EpochEvent]) -> Vec<(Epoch, SubstateAddress)> {
        events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::NewValidatorRegistered {
                    activation_epoch,
                    shard_key,
                    ..
                } => Some((*activation_epoch, *shard_key)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_pending_registration_is_announced_before_the_first_tick() {
        let validator = mk_validator(1, Epoch(10));
        let shard_key = validator.calculate_shard_key();
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(
            mk_config(vec![validator]),
            TestStore::default(),
            ticker_through(0),
        );

        let events = drive_events(&mut oracle);

        assert!(
            matches!(events.first(), Some(EpochEvent::NewValidatorRegistered { .. })),
            "the registration must be announced ahead of every epoch change; got {events:?}"
        );
        assert_eq!(announced_registrations(&events), vec![(Epoch(11), shard_key)]);
    }

    #[test]
    fn restart_before_activation_announces_the_pending_registration_again() {
        let validator = mk_validator(1, Epoch(10));
        let shard_key = validator.calculate_shard_key();
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(5))
            .unwrap();
        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(5),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(mk_config(vec![validator]), store, ticker);

        let events = drive_events(&mut oracle);

        assert_eq!(announced_registrations(&events), vec![(Epoch(11), shard_key)]);
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

    fn mk_validator_with_rotation(registration_epoch: Epoch, effective_epoch: Epoch) -> Validator {
        Validator {
            claim_key_changes: vec![ClaimKeyChange {
                effective_epoch,
                claim_key: mk_key(99),
            }],
            ..mk_validator(1, registration_epoch)
        }
    }

    #[test]
    fn claim_key_rotation_re_adds_validator_with_new_key_and_same_shard_key() {
        let validator = mk_validator_with_rotation(Epoch(10), Epoch(20));
        let expected_shard_key = validator.calculate_shard_key();
        let original_claim_key = validator.claim_key;
        let config = mk_config(vec![validator]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker_through(20));

        let events = drive_events(&mut oracle);

        assert_eq!(
            added_validators(&events),
            vec![
                (Epoch(10), original_claim_key, Epoch(11), expected_shard_key),
                (Epoch(19), mk_key(99), Epoch(20), expected_shard_key),
            ],
            "rotation should re-add the validator at its effective epoch with the new claim key and the \
             registration's shard key"
        );
    }

    #[test]
    fn claim_key_rotation_is_not_announced_as_a_new_registration() {
        let config = mk_config(vec![mk_validator_with_rotation(Epoch(10), Epoch(20))]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker_through(20));

        let events = drive_events(&mut oracle);

        let announced = events
            .iter()
            .filter(|e| matches!(e, EpochEvent::NewValidatorRegistered { .. }))
            .count();
        assert_eq!(
            announced, 1,
            "only the initial registration should be announced; got {events:?}"
        );
    }

    #[test]
    fn restart_before_effective_epoch_still_emits_the_rotation() {
        let config = mk_config(vec![mk_validator_with_rotation(Epoch(10), Epoch(20))]);
        let store = TestStore::default();
        // A prior run activated the validator but has not yet reached the rotation.
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(15))
            .unwrap();
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker_through(20));

        let events = drive_events(&mut oracle);

        assert_eq!(
            added_validators(&events)
                .into_iter()
                .map(|(_, claim_key, activation_epoch, _)| (claim_key, activation_epoch))
                .collect::<Vec<_>>(),
            vec![(mk_key(99), Epoch(20))],
            "the already-activated registration must not be re-emitted, but the pending rotation must be"
        );
    }

    #[test]
    fn restart_after_effective_epoch_does_not_re_emit_the_rotation() {
        let config = mk_config(vec![mk_validator_with_rotation(Epoch(10), Epoch(20))]);
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(25))
            .unwrap();
        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(25),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker);

        let events = drive_events(&mut oracle);

        assert!(
            added_validators(&events).is_empty(),
            "an already-applied rotation must not be re-emitted; got {events:?}"
        );
    }

    #[test]
    fn fresh_store_replay_past_the_rotation_converges_on_the_new_key() {
        let config = mk_config(vec![mk_validator_with_rotation(Epoch(10), Epoch(20))]);
        // A node syncing from scratch replays every epoch, so it must arrive at the same final claim key as a node
        // that was live across the boundary.
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker_through(30));

        let events = drive_events(&mut oracle);

        let (_, final_claim_key, final_activation_epoch, _) =
            *added_validators(&events).last().expect("at least one Add");
        assert_eq!(final_claim_key, mk_key(99));
        assert_eq!(final_activation_epoch, Epoch(20));
    }

    #[test]
    fn editing_the_claim_key_of_an_activated_validator_is_rejected() {
        let store = TestStore::default();
        // A prior run activated the validator and pinned the shard key it derived.
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(15))
            .unwrap();
        let original = mk_validator(1, Epoch(10));
        store
            .set(StoreKey::ConfiguredValidatorShardKeys.as_key_bytes(), &vec![(
                original.public_key,
                original.calculate_shard_key(),
            )])
            .unwrap();

        // The operator rotates by editing `claim_key` in place rather than adding a `claim_key_changes` entry. This
        // is silently a no-op here but would derive a different shard key on a node replaying from a fresh store.
        let edited = Validator {
            claim_key: mk_key(99),
            ..original
        };
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(mk_config(vec![edited]), store, ticker_through(20));

        let events = drive_events(&mut oracle);

        match events.as_slice() {
            [EpochEvent::Error(err)] => {
                let err = err.to_string();
                assert!(err.contains("activated with shard key"), "{err}");
            },
            _ => panic!("editing an activated validator's claim key must be rejected; got {events:?}"),
        }
    }

    #[test]
    fn a_scheduled_rotation_does_not_trip_the_shard_key_check() {
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(15))
            .unwrap();
        let validator = mk_validator_with_rotation(Epoch(10), Epoch(20));
        store
            .set(StoreKey::ConfiguredValidatorShardKeys.as_key_bytes(), &vec![(
                validator.public_key,
                // Pinned from the config before the rotation was added.
                mk_validator(1, Epoch(10)).calculate_shard_key(),
            )])
            .unwrap();
        let mut oracle =
            ConfiguredEpochOracle::with_custom_ticker(mk_config(vec![validator]), store, ticker_through(20));

        let events = drive_events(&mut oracle);

        assert!(
            !events.iter().any(|e| matches!(e, EpochEvent::Error(_))),
            "adding a claim key rotation must not change the shard key; got {events:?}"
        );
        assert_eq!(
            added_validators(&events)
                .into_iter()
                .map(|(_, claim_key, _, _)| claim_key)
                .collect::<Vec<_>>(),
            vec![mk_key(99)]
        );
    }

    #[test]
    fn invalid_config_stops_the_oracle_with_an_error() {
        let mut validator = mk_validator(1, Epoch(10));
        // Rotating before the validator activates has no row to supersede.
        validator.claim_key_changes = vec![ClaimKeyChange {
            effective_epoch: Epoch(5),
            claim_key: mk_key(99),
        }];
        let config = mk_config(vec![validator]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker_through(20));

        let events = drive_events(&mut oracle);

        assert!(
            matches!(events.as_slice(), [EpochEvent::Error(_)]),
            "an invalid config must surface as an error, not a partial event stream; got {events:?}"
        );
    }

    fn mk_exiting_validator(public_seed: u8, registration_epoch: Epoch, exit_epoch: Epoch) -> Validator {
        Validator {
            exit_epoch: Some(exit_epoch),
            ..mk_validator(public_seed, registration_epoch)
        }
    }

    /// Each `Remove` change as (event epoch, public key), and each `EpochChanged` as (epoch, None), in emission order.
    fn exits_and_epoch_changes(events: &[EpochEvent]) -> Vec<(Epoch, Option<RistrettoPublicKeyBytes>)> {
        events
            .iter()
            .flat_map(|e| match e {
                EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => node_changes
                    .iter()
                    .filter_map(|change| match change {
                        ValidatorNodeChange::Remove { public_key } => Some((*epoch, Some(*public_key))),
                        ValidatorNodeChange::Add { .. } => None,
                    })
                    .collect(),
                EpochEvent::EpochChanged { epoch, .. } => vec![(*epoch, None)],
                _ => vec![],
            })
            .collect()
    }

    fn removed_validators(events: &[EpochEvent]) -> Vec<(Epoch, RistrettoPublicKeyBytes)> {
        exits_and_epoch_changes(events)
            .into_iter()
            .filter_map(|(epoch, public_key)| public_key.map(|pk| (epoch, pk)))
            .collect()
    }

    #[test]
    fn an_exit_is_emitted_for_its_exit_epoch_before_that_epoch_changes() {
        let config = mk_config(vec![mk_exiting_validator(1, Epoch(1), Epoch(5))]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker_through(6));

        let events = drive_events(&mut oracle);

        let sequence = exits_and_epoch_changes(&events);
        let exit_at = sequence
            .iter()
            .position(|entry| *entry == (Epoch(5), Some(mk_key(1))))
            .unwrap_or_else(|| panic!("expected a Remove for epoch 5; got {events:?}"));
        assert_eq!(sequence.get(exit_at - 1), Some(&(Epoch(4), None)));
        assert_eq!(sequence.get(exit_at + 1), Some(&(Epoch(5), None)));
        assert_eq!(removed_validators(&events), vec![(Epoch(5), mk_key(1))]);
    }

    #[test]
    fn restart_before_the_exit_epoch_still_emits_the_exit() {
        let config = mk_config(vec![mk_exiting_validator(1, Epoch(1), Epoch(5))]);
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(3))
            .unwrap();
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker_through(6));

        let events = drive_events(&mut oracle);

        assert_eq!(removed_validators(&events), vec![(Epoch(5), mk_key(1))]);
    }

    #[test]
    fn restart_after_the_exit_epoch_does_not_re_emit_the_exit() {
        let config = mk_config(vec![mk_exiting_validator(1, Epoch(1), Epoch(5))]);
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(5))
            .unwrap();
        store
            .set(StoreKey::ConfiguredAppliedExits.as_key_bytes(), &vec![(
                mk_key(1),
                Epoch(5),
            )])
            .unwrap();
        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(5),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker);

        let events = drive_events(&mut oracle);

        assert!(
            removed_validators(&events).is_empty(),
            "an already-applied exit must not be re-emitted; got {events:?}"
        );
    }

    #[test]
    fn an_applied_exit_is_recorded_and_verified_on_restart() {
        let config = mk_config(vec![mk_exiting_validator(1, Epoch(1), Epoch(5))]);
        let store = TestStore::default();
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config.clone(), store, ticker_through(6));
        drive_events(&mut oracle);

        let store = oracle.store;
        assert_eq!(
            store
                .get::<Vec<(RistrettoPublicKeyBytes, Epoch)>>(StoreKey::ConfiguredAppliedExits.as_key_bytes())
                .unwrap(),
            Some(vec![(mk_key(1), Epoch(5))])
        );
        let mut restarted = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker_from(6, 6));
        let events = drive_events(&mut restarted);
        assert!(
            !events.iter().any(|e| matches!(e, EpochEvent::Error(_))),
            "got {events:?}"
        );
    }

    #[test]
    fn an_exit_received_after_its_epoch_stops_the_oracle() {
        // A prior run passed epoch 5 under a config without the exit.
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(6))
            .unwrap();
        let config = mk_config(vec![mk_exiting_validator(1, Epoch(1), Epoch(5))]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker_from(6, 6));

        let events = drive_events(&mut oracle);

        match events.as_slice() {
            [EpochEvent::Error(err)] => {
                let err = err.to_string();
                assert!(err.contains("Resync it from a fresh epoch oracle store"), "{err}");
            },
            _ => panic!("a late exit must stop the oracle; got {events:?}"),
        }
    }

    #[test]
    fn an_applied_exit_dropped_from_the_config_stops_the_oracle() {
        let store = TestStore::default();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(6))
            .unwrap();
        store
            .set(StoreKey::ConfiguredAppliedExits.as_key_bytes(), &vec![(
                mk_key(1),
                Epoch(5),
            )])
            .unwrap();
        let config = mk_config(vec![mk_validator(1, Epoch(1))]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker_from(6, 6));

        let events = drive_events(&mut oracle);

        match events.as_slice() {
            [EpochEvent::Error(err)] => {
                let err = err.to_string();
                assert!(err.contains("does not hold that exit"), "{err}");
            },
            _ => panic!("dropping an applied exit must stop the oracle; got {events:?}"),
        }
    }

    fn ticker_from(first_epoch: u64, last_epoch: u64) -> ScriptedTicker {
        ScriptedTicker::new(
            (first_epoch..=last_epoch)
                .map(|e| EpochTickerData {
                    epoch: Epoch(e),
                    epoch_hash: FixedHash::zero(),
                    done_for_now: e == last_epoch,
                })
                .collect(),
        )
    }

    #[test]
    fn skipped_epochs_activate_a_validator_before_its_exit() {
        let config = mk_config(vec![mk_exiting_validator(1, Epoch(2), Epoch(5))]);
        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(10),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker);

        let events = drive_events(&mut oracle);

        let changes = events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => Some((*epoch, node_changes)),
                _ => None,
            })
            .flat_map(|(epoch, node_changes)| {
                node_changes.iter().map(move |change| match change {
                    ValidatorNodeChange::Add { activation_epoch, .. } => ("add", *activation_epoch),
                    ValidatorNodeChange::Remove { .. } => ("remove", epoch),
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(changes, vec![("add", Epoch(3)), ("remove", Epoch(5))]);
    }

    fn following(
        config: Config,
        store: TestStore,
        ticker: ScriptedTicker,
    ) -> (
        ConfiguredEpochOracle<TestStore, ScriptedTicker>,
        tokio::sync::mpsc::UnboundedSender<Option<Config>>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let oracle = ConfiguredEpochOracle::with_custom_ticker(config, store, ticker).with_config_updates(rx);
        (oracle, tx)
    }

    fn epoch_changes(events: &[EpochEvent]) -> Vec<Epoch> {
        events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::EpochChanged { epoch, .. } => Some(*epoch),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_following_oracle_holds_epochs_until_the_first_update() {
        let config = mk_config(vec![mk_validator(1, Epoch(1))]);
        let (mut oracle, tx) = following(config, TestStore::default(), ticker_through(3));

        let events = drive_events(&mut oracle);
        assert!(epoch_changes(&events).is_empty(), "got {events:?}");

        // No config could be fetched: the epochs go ahead under the config the oracle holds.
        tx.send(None).unwrap();
        let events = drive_events(&mut oracle);
        assert_eq!(epoch_changes(&events), vec![Epoch(0), Epoch(1), Epoch(2), Epoch(3)]);
    }

    #[test]
    fn a_first_update_from_a_fresh_store_replays_its_whole_validator_set() {
        let release = mk_config(vec![mk_validator(1, Epoch(1))]);
        let network = mk_config(vec![
            mk_exiting_validator(1, Epoch(1), Epoch(5)),
            mk_validator(2, Epoch(2)),
        ]);
        let (mut oracle, tx) = following(release, TestStore::default(), ticker_through(6));
        tx.send(Some(network)).unwrap();

        let events = drive_events(&mut oracle);

        assert_eq!(added_or_removed(&events), vec![
            (Epoch(2), mk_key(1), true),
            (Epoch(3), mk_key(2), true),
            (Epoch(5), mk_key(1), false)
        ]);
    }

    #[test]
    fn an_update_schedules_an_exit_after_the_processed_epoch() {
        let config = mk_config(vec![mk_validator(1, Epoch(1)), mk_validator(2, Epoch(1))]);
        let (mut oracle, tx) = following(config.clone(), TestStore::default(), ticker_through(3));
        tx.send(Some(config.clone())).unwrap();
        drive_events(&mut oracle);

        let mut updated = config;
        updated.validators[0].exit_epoch = Some(Epoch(5));
        tx.send(Some(updated)).unwrap();
        oracle.ticker = ticker_from(4, 6);
        let events = drive_events(&mut oracle);

        assert_eq!(removed_validators(&events), vec![(Epoch(5), mk_key(1))]);
    }

    #[test]
    fn an_update_adding_a_new_validator_announces_it() {
        let config = mk_config(vec![mk_validator(1, Epoch(1))]);
        let (mut oracle, tx) = following(config.clone(), TestStore::default(), ticker_through(3));
        tx.send(Some(config.clone())).unwrap();
        drive_events(&mut oracle);

        let joining = mk_validator(2, Epoch(5));
        let shard_key = joining.calculate_shard_key();
        let mut updated = config;
        updated.validators.push(joining);
        tx.send(Some(updated)).unwrap();
        oracle.ticker = ticker_from(4, 6);
        let events = drive_events(&mut oracle);

        assert_eq!(announced_registrations(&events), vec![(Epoch(6), shard_key)]);
        assert_eq!(
            added_or_removed(&events),
            vec![(Epoch(6), mk_key(2), true)],
            "only the new validator activates"
        );
    }

    #[test]
    fn an_update_rescheduling_a_pending_exit_moves_it() {
        let config = mk_config(vec![
            mk_validator(1, Epoch(1)),
            mk_exiting_validator(2, Epoch(1), Epoch(5)),
        ]);
        let (mut oracle, tx) = following(config.clone(), TestStore::default(), ticker_through(3));
        tx.send(Some(config.clone())).unwrap();
        drive_events(&mut oracle);

        let mut rescheduled = config;
        rescheduled.validators[1].exit_epoch = Some(Epoch(7));
        tx.send(Some(rescheduled)).unwrap();
        oracle.ticker = ticker_from(4, 8);
        let events = drive_events(&mut oracle);

        assert_eq!(removed_validators(&events), vec![(Epoch(7), mk_key(2))]);
    }

    #[test]
    fn an_update_changing_a_processed_epoch_is_ignored() {
        let config = mk_config(vec![mk_validator(1, Epoch(1)), mk_validator(2, Epoch(1))]);
        let (mut oracle, tx) = following(config.clone(), TestStore::default(), ticker_through(3));
        tx.send(Some(config.clone())).unwrap();
        drive_events(&mut oracle);

        let mut updated = config.clone();
        updated.validators[0].exit_epoch = Some(Epoch(3));
        tx.send(Some(updated)).unwrap();
        oracle.ticker = ticker_from(4, 6);
        let events = drive_events(&mut oracle);

        assert!(removed_validators(&events).is_empty(), "got {events:?}");
        assert_eq!(oracle.config, config);
    }

    #[test]
    fn a_restarted_following_oracle_resumes_the_adopted_config() {
        let release = mk_config(vec![mk_validator(1, Epoch(1)), mk_validator(2, Epoch(1))]);
        let store = TestStore::default();
        let mut adopted = release.clone();
        adopted.validators[0].exit_epoch = Some(Epoch(5));
        store
            .set(StoreKey::ConfiguredFollowedConfig.as_key_bytes(), &adopted)
            .unwrap();
        store
            .set(StoreKey::ConfiguredCurrentEpoch.as_key_bytes(), &Epoch(3))
            .unwrap();

        // The release config is older than the adopted one, and no validator served a config at startup.
        let (mut oracle, tx) = following(release, store, ticker_from(3, 6));
        tx.send(None).unwrap();
        let events = drive_events(&mut oracle);

        assert_eq!(removed_validators(&events), vec![(Epoch(5), mk_key(1))]);
        assert_eq!(oracle.config, adopted);
    }

    #[test]
    fn a_non_following_oracle_ignores_a_stored_followed_config() {
        let release = mk_config(vec![mk_validator(1, Epoch(1))]);
        let store = TestStore::default();
        let mut adopted = release.clone();
        adopted.validators[0].exit_epoch = Some(Epoch(5));
        store
            .set(StoreKey::ConfiguredFollowedConfig.as_key_bytes(), &adopted)
            .unwrap();
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(release, store, ticker_through(6));

        let events = drive_events(&mut oracle);

        assert!(removed_validators(&events).is_empty(), "got {events:?}");
    }

    /// Each change as (effective epoch, public key, is_add), where an `Add` takes effect at its activation epoch and a
    /// `Remove` at its event's epoch.
    fn added_or_removed(events: &[EpochEvent]) -> Vec<(Epoch, RistrettoPublicKeyBytes, bool)> {
        events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => Some((*epoch, node_changes)),
                _ => None,
            })
            .flat_map(|(epoch, node_changes)| {
                node_changes.iter().map(move |change| match change {
                    ValidatorNodeChange::Add {
                        validator_node_public_key,
                        activation_epoch,
                        ..
                    } => (*activation_epoch, *validator_node_public_key, true),
                    ValidatorNodeChange::Remove { public_key } => (epoch, *public_key, false),
                })
            })
            .collect()
    }

    #[test]
    fn epoch_zero_emits_done_for_now_when_ticker_is_done() {
        let config = mk_config(vec![]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker_through(0));

        let events = drive_events(&mut oracle);

        assert!(
            matches!(events.as_slice(), [
                EpochEvent::EpochChanged { epoch: Epoch(0), .. },
                EpochEvent::DoneForNow { epoch: Epoch(0), .. }
            ]),
            "epoch 0 must emit DoneForNow when done_for_now is true; got {events:?}"
        );
    }

    #[test]
    fn skipped_epochs_preserve_configured_activation_epoch_and_epoch_zero_emits_done_for_now() {
        let v1 = mk_validator(1, Epoch(2));
        let v2 = mk_validator(2, Epoch(4));
        let config = mk_config(vec![v1.clone(), v2.clone()]);
        let ticker = ScriptedTicker::new(vec![
            EpochTickerData {
                epoch: Epoch(0),
                epoch_hash: FixedHash::zero(),
                done_for_now: true,
            },
            EpochTickerData {
                epoch: Epoch(10),
                epoch_hash: FixedHash::zero(),
                done_for_now: true,
            },
        ]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker);

        let events = drive_events(&mut oracle);

        assert!(
            events
                .iter()
                .any(|e| matches!(e, EpochEvent::DoneForNow { epoch, .. } if *epoch == Epoch(0))),
            "epoch 0 with done_for_now=true must emit DoneForNow; got {events:?}"
        );
        assert_eq!(
            added_validators(&events)
                .into_iter()
                .map(|(_, claim_key, activation_epoch, _)| (claim_key, activation_epoch))
                .collect::<Vec<_>>(),
            vec![(v1.claim_key, Epoch(3)), (v2.claim_key, Epoch(5))],
            "validators activated across skipped epochs must retain their configured activation_epoch"
        );
    }

    #[test]
    fn registration_and_rotation_carry_the_configured_power() {
        let heavy = Validator {
            power: NonZeroU64::new(5).unwrap(),
            ..mk_validator_with_rotation(Epoch(10), Epoch(20))
        };
        let light = mk_validator(3, Epoch(10));
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(
            mk_config(vec![heavy, light]),
            TestStore::default(),
            ticker_through(20),
        );

        let events = drive_events(&mut oracle);

        let mut powers = events
            .iter()
            .filter_map(|e| match e {
                EpochEvent::ActiveValidatorNodeSetChanged { node_changes, .. } => Some(node_changes),
                _ => None,
            })
            .flatten()
            .map(|change| match change {
                ValidatorNodeChange::Add {
                    validator_node_public_key,
                    activation_epoch,
                    vote_power,
                    ..
                } => (*activation_epoch, *validator_node_public_key, *vote_power),
                ValidatorNodeChange::Remove { .. } => panic!("unexpected Remove change"),
            })
            .collect::<Vec<_>>();
        powers.sort();
        let mut expected = vec![
            (Epoch(11), mk_key(1), VotePower::of(5)),
            (Epoch(11), mk_key(3), VotePower::of(1)),
            (Epoch(20), mk_key(1), VotePower::of(5)),
        ];
        expected.sort();
        assert_eq!(powers, expected);
    }

    #[test]
    fn current_epoch_is_persisted_only_after_epoch_events_are_drained() {
        let config = mk_config(vec![mk_validator(1, Epoch(0))]);
        let ticker = ScriptedTicker::new(vec![EpochTickerData {
            epoch: Epoch(1),
            epoch_hash: FixedHash::zero(),
            done_for_now: true,
        }]);
        let mut oracle = ConfiguredEpochOracle::with_custom_ticker(config, TestStore::default(), ticker);
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // First poll yields NewValidatorRegistered from initialize().
        assert!(matches!(
            oracle.poll(&mut cx),
            Poll::Ready(Some(EpochEvent::NewValidatorRegistered { .. }))
        ));
        // Second poll triggers prepare_next_epoch(Epoch(1)) and yields ActiveValidatorNodeSetChanged.
        assert!(matches!(
            oracle.poll(&mut cx),
            Poll::Ready(Some(EpochEvent::ActiveValidatorNodeSetChanged { .. }))
        ));
        // Third and fourth polls yield EpochChanged and DoneForNow.
        assert!(matches!(
            oracle.poll(&mut cx),
            Poll::Ready(Some(EpochEvent::EpochChanged { epoch: Epoch(1), .. }))
        ));
        assert!(matches!(
            oracle.poll(&mut cx),
            Poll::Ready(Some(EpochEvent::DoneForNow { epoch: Epoch(1), .. }))
        ));
        assert_eq!(
            oracle
                .store
                .get::<Epoch>(StoreKey::ConfiguredCurrentEpoch.as_key_bytes())
                .unwrap(),
            None,
            "epoch must not be persisted until the caller polls again after consuming the last event"
        );

        // Next poll after consuming DoneForNow persists Epoch(1) before waiting on the ticker.
        assert!(matches!(oracle.poll(&mut cx), Poll::Pending));
        assert_eq!(
            oracle
                .store
                .get::<Epoch>(StoreKey::ConfiguredCurrentEpoch.as_key_bytes())
                .unwrap(),
            Some(Epoch(1))
        );
    }
}
