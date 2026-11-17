//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::BTreeSet,
    hash::{BuildHasher, RandomState},
    time::Duration,
};

use anyhow::Context;
use log::*;
use ootle_byte_type::FromByteType;
use tari_epoch_manager::{EpochManagerEvent, EpochManagerReader, service::EpochManagerHandle};
use tari_epoch_oracles::configured::Config;
use tari_networking::{NetworkingHandle, NetworkingService};
use tari_ootle_common_types::DerivableFromPublicKey;
use tari_ootle_p2p::{PeerAddress, TariMessagingSpec, ToPeerId, proto::rpc::GetEpochOracleConfigRequest};
use tari_shutdown::ShutdownSignal;
use tari_validator_node_rpc::client::RpcMultiPool;
use tokio::{
    sync::{broadcast::error::RecvError, mpsc},
    time,
};

const LOG_TARGET: &str = "tari::indexer::validator_oracle_config";

/// How long startup waits for a validator's config before the epoch oracle goes ahead with the config it holds.
const STARTUP_FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const STARTUP_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// The validators asked for their config in one attempt, one after another until enough of them agree.
const MAX_VALIDATORS_PER_ATTEMPT: usize = 5;
/// How many validators must serve the same config before it is sent to the oracle, so that a single misconfigured
/// validator cannot move every indexer that asks it. Networks with fewer validators need all of them.
const REQUIRED_AGREEMENT: usize = 2;

/// Fetches the configured epoch oracle's config from validators and sends it to this indexer's oracle, which adopts
/// it if it agrees with what the oracle has already processed.
///
/// It fetches once at startup, while the oracle holds back epoch changes, so that an indexer that was offline when a
/// validator joined or exited still applies the change in order; and then once per epoch.
pub struct ValidatorOracleConfigFollower {
    epoch_manager: EpochManagerHandle<PeerAddress>,
    networking: NetworkingHandle<TariMessagingSpec>,
    pool: RpcMultiPool<TariMessagingSpec>,
    updates: mpsc::UnboundedSender<Option<Config>>,
    /// The validators of the config this indexer started with, asked until the epoch manager knows the current
    /// validators.
    bootstrap_validators: Vec<PeerAddress>,
    last_sent: Option<Config>,
    /// Where the next attempt starts in the candidate list. It starts at a random offset so that indexers spread
    /// their first requests over the validators, and advances so that successive attempts ask different ones.
    next_offset: usize,
}

impl ValidatorOracleConfigFollower {
    pub fn new(
        epoch_manager: EpochManagerHandle<PeerAddress>,
        networking: NetworkingHandle<TariMessagingSpec>,
        updates: mpsc::UnboundedSender<Option<Config>>,
        started_with: &Config,
    ) -> Self {
        let bootstrap_validators = started_with
            .validators
            .iter()
            .filter_map(|vn| vn.public_key.try_from_byte_type().ok())
            .map(|public_key| PeerAddress::derive_from_public_key(&public_key))
            .collect();
        Self {
            epoch_manager,
            pool: RpcMultiPool::new(networking.clone()),
            networking,
            updates,
            bootstrap_validators,
            last_sent: None,
            next_offset: usize::try_from(RandomState::new().hash_one(0u8) % u64::from(u32::MAX)).unwrap_or_default(),
        }
    }

    pub async fn run(mut self, mut shutdown: ShutdownSignal) {
        let mut events = self.epoch_manager.subscribe();

        // The epoch manager sets the wanted peers on each epoch change, which the oracle holds back until this
        // first fetch resolves, so a fresh indexer asks peer sync for the validators it started with.
        if let Err(err) = self
            .networking
            .set_want_peers(self.bootstrap_validators.iter().map(|vn| vn.to_peer_id()))
            .await
        {
            warn!(target: LOG_TARGET, "Failed to request the configured validators' peer records: {err}");
        }

        let started = time::Instant::now();
        loop {
            match self.fetch_and_send().await {
                FetchOutcome::Sent => break,
                FetchOutcome::OracleGone => return,
                FetchOutcome::NoAgreement => {},
            }
            if started.elapsed() >= STARTUP_FETCH_TIMEOUT {
                warn!(
                    target: LOG_TARGET,
                    "Validators did not agree on an epoch oracle config within {STARTUP_FETCH_TIMEOUT:.0?}; \
                     continuing with the config this indexer holds and retrying each epoch"
                );
                if self.updates.send(None).is_err() {
                    return;
                }
                break;
            }
            tokio::select! {
                _ = time::sleep(STARTUP_RETRY_INTERVAL) => {},
                _ = shutdown.wait() => return,
            }
        }

        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Ok(EpochManagerEvent::EpochChanged { .. }) | Err(RecvError::Lagged(_)) => {
                        if matches!(self.fetch_and_send().await, FetchOutcome::OracleGone) {
                            return;
                        }
                    },
                    Ok(_) => {},
                    Err(RecvError::Closed) => return,
                },
                _ = shutdown.wait() => return,
            }
        }
    }

    /// Fetches the config from a few validators and sends the first one enough of them agree on, unless it was the
    /// last one sent.
    async fn fetch_and_send(&mut self) -> FetchOutcome {
        let candidates = self.candidates().await;
        let required = REQUIRED_AGREEMENT.min(candidates.len()).max(1);
        let mut served = Vec::<(Config, usize)>::new();
        for validator in candidates.iter().take(MAX_VALIDATORS_PER_ATTEMPT) {
            let config = match self.fetch_from(validator).await {
                Ok(Some(config)) => config,
                Ok(None) => {
                    debug!(target: LOG_TARGET, "Validator {validator} runs no configured epoch oracle");
                    continue;
                },
                Err(err) => {
                    debug!(target: LOG_TARGET, "Failed to fetch the epoch oracle config from {validator}: {err:#}");
                    continue;
                },
            };
            let count = match served.iter_mut().find(|(seen, _)| *seen == config) {
                Some((_, count)) => {
                    *count += 1;
                    *count
                },
                None => {
                    served.push((config.clone(), 1));
                    1
                },
            };
            if count < required {
                continue;
            }
            if self.last_sent.as_ref() != Some(&config) {
                debug!(target: LOG_TARGET, "{count} validator(s) served the same epoch oracle config update");
                if self.updates.send(Some(config.clone())).is_err() {
                    return FetchOutcome::OracleGone;
                }
                self.last_sent = Some(config);
            }
            return FetchOutcome::Sent;
        }
        if served.len() > 1 {
            warn!(
                target: LOG_TARGET,
                "Validators served {} different epoch oracle configs; none was adopted",
                served.len()
            );
        }
        FetchOutcome::NoAgreement
    }

    /// The current epoch's validators, falling back to the bootstrap validators, starting from a different one on
    /// each attempt.
    async fn candidates(&mut self) -> Vec<PeerAddress> {
        let mut candidates = BTreeSet::new();
        if let Ok(epoch) = self.epoch_manager.current_epoch().await &&
            let Ok(validators) = self.epoch_manager.get_all_validator_nodes(epoch).await
        {
            candidates.extend(validators.into_iter().map(|vn| vn.address));
        }
        if candidates.is_empty() {
            candidates.extend(self.bootstrap_validators.iter().copied());
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if !candidates.is_empty() {
            let len = candidates.len();
            candidates.rotate_left(self.next_offset % len);
        }
        self.next_offset = self.next_offset.wrapping_add(MAX_VALIDATORS_PER_ATTEMPT);
        candidates
    }

    async fn fetch_from(&self, validator: &PeerAddress) -> anyhow::Result<Option<Config>> {
        let mut client = self.pool.get_or_connect(&validator.to_peer_id()).await?;
        let response = client.get_epoch_oracle_config(GetEpochOracleConfigRequest {}).await?;
        if response.config_json.is_empty() {
            return Ok(None);
        }
        let config = serde_json::from_str::<Config>(&response.config_json)
            .context("The validator served an epoch oracle config that is not valid JSON")?;
        config.validate()?;
        Ok(Some(config))
    }
}

enum FetchOutcome {
    /// Enough validators agreed on a config, which the oracle has, or was just sent.
    Sent,
    /// Too few validators answered, or they disagreed.
    NoAgreement,
    /// The oracle has stopped and takes no more updates.
    OracleGone,
}
