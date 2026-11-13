//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeSet, time::Duration};

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
/// The validators asked for their config in one attempt, one after another until one answers.
const MAX_VALIDATORS_PER_ATTEMPT: usize = 3;

/// Fetches the configured epoch oracle's config from validators and sends it to this indexer's oracle, which adopts
/// it if it only schedules changes for epochs the oracle has not processed.
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
    /// Advanced on each attempt so that successive attempts start with different validators.
    attempts: usize,
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
            attempts: 0,
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
        while !self.fetch_and_send().await {
            if started.elapsed() >= STARTUP_FETCH_TIMEOUT {
                warn!(
                    target: LOG_TARGET,
                    "No validator served its epoch oracle config within {STARTUP_FETCH_TIMEOUT:.0?}; continuing with \
                     the config this indexer holds and retrying each epoch"
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
                        self.fetch_and_send().await;
                    },
                    Ok(_) => {},
                    Err(RecvError::Closed) => return,
                },
                _ = shutdown.wait() => return,
            }
        }
    }

    /// Fetches the config from a few validators, sending the first one served if it differs from the last sent.
    /// Returns true once a validator has served a config.
    async fn fetch_and_send(&mut self) -> bool {
        let candidates = self.candidates().await;
        for validator in candidates.iter().take(MAX_VALIDATORS_PER_ATTEMPT) {
            match self.fetch_from(validator).await {
                Ok(Some(config)) => {
                    if self.last_sent.as_ref() != Some(&config) {
                        debug!(target: LOG_TARGET, "Validator {validator} served an epoch oracle config update");
                        if self.updates.send(Some(config.clone())).is_err() {
                            return true;
                        }
                        self.last_sent = Some(config);
                    }
                    return true;
                },
                Ok(None) => {
                    debug!(target: LOG_TARGET, "Validator {validator} runs no configured epoch oracle");
                },
                Err(err) => {
                    debug!(target: LOG_TARGET, "Failed to fetch the epoch oracle config from {validator}: {err:#}");
                },
            }
        }
        false
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
            candidates.rotate_left(self.attempts % len);
        }
        self.attempts = self.attempts.wrapping_add(MAX_VALIDATORS_PER_ATTEMPT);
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
