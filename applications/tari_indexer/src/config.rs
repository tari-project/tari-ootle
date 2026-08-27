//  Copyright 2023. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use config::Config;
use ootle_byte_type::ToByteType;
use serde::{Deserialize, Serialize};
use tari_common::{
    ConfigurationError,
    DefaultConfigLoader,
    SubConfigPath,
    configuration::{CommonConfig, serializers},
};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_app_utilities::{
    epoch_oracle_config::EpochOracleConfig,
    p2p_config::{P2pConfig, PeerSeedsConfig},
};
use tari_ootle_transaction::Network;
use tari_template_lib_types::{TemplateAddress, crypto::RistrettoPublicKeyBytes};

use crate::{network_state_sync::EventFilter, rest_api::RefillRate};

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub common: CommonConfig,
    pub indexer: IndexerConfig,
    pub peer_seeds: PeerSeedsConfig,
    pub epoch_oracle: EpochOracleConfig,
    pub network: Network,
}

impl ApplicationConfig {
    pub fn load_from(cfg: &Config) -> Result<Self, ConfigurationError> {
        let config = Self {
            common: CommonConfig::load_from(cfg)?,
            indexer: IndexerConfig::load_from(cfg)?,
            peer_seeds: PeerSeedsConfig::load_from(cfg)?,
            epoch_oracle: EpochOracleConfig::load_from(cfg)?,
            network: cfg.get("network")?,
        };
        Ok(config)
    }

    pub fn to_identity_file_path(&self) -> PathBuf {
        if self.indexer.identity_file.is_absolute() {
            return self.indexer.identity_file.clone();
        }

        self.common.base_path.join(&self.indexer.identity_file)
    }

    pub fn to_data_dir(&self) -> PathBuf {
        if self.indexer.data_dir.is_absolute() {
            return self.indexer.data_dir.clone();
        }

        self.common.base_path.join(&self.indexer.data_dir)
    }

    pub fn state_db_path(&self) -> PathBuf {
        self.to_data_dir().join("state.db")
    }

    pub fn global_db_path(&self) -> PathBuf {
        self.to_data_dir().join("global_storage.sqlite")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct IndexerConfig {
    override_from: Option<String>,
    /// A path to the file that stores your node identity and secret key
    pub identity_file: PathBuf,
    /// The relative path to store persistent data
    pub data_dir: PathBuf,
    /// The p2p configuration settings
    pub p2p: P2pConfig,
    /// Listening address for the indexer API server
    pub api_listen_address: Option<SocketAddr>,
    /// Listening address for the Prometheus metrics endpoint (`/_metrics`). This is served on a dedicated listener,
    /// separate from the API server, so that metrics can be bound to a private interface while the API is public.
    /// Only used when the `metrics` feature is enabled (default = "127.0.0.1:18302"); `None` disables the listener.
    pub metrics_listen_address: Option<SocketAddr>,
    /// GraphQL port of the indexer application
    pub graphql_address: Option<SocketAddr>,
    /// The address of the Web UI
    pub web_ui_address: Option<SocketAddr>,
    /// The publicly-accessible URL that the UI uses to connect to the API.
    /// If this is None, then the api_listen_address will be used.
    pub web_ui_public_api_url: Option<String>,
    /// The jrpc address where the UI should connect to the GraphQL API(it can be the same as the json_rpc_address, but
    /// doesn't have to be), if this will be None, then the listen_addr will be used.
    pub web_ui_public_graphql_url: Option<String>,
    /// How often do we want to scan the second layer for new versions
    #[serde(with = "serializers::seconds")]
    pub block_scanning_interval: Duration,
    #[serde(with = "serializers::seconds")]
    pub state_scanning_interval: Duration,
    /// The sidechain to listen on. Also identifies this chain for L1 burn-claim binding.
    pub sidechain_id: Option<RistrettoPublicKey>,
    /// Cache TTL for substates fetched during dry run transaction processing.
    /// A shorter TTL reduces the chance of stale fee estimates.
    #[serde(with = "serializers::seconds")]
    pub dry_run_cache_ttl: Duration,
    /// Start even when the binary's schema activation schedule disagrees with the one this node has
    /// already run under. Doing so re-hashes committed state and breaks the substate proofs this
    /// indexer serves, so this exists only for a node whose state is being discarded.
    #[serde(default)]
    pub allow_past_protocol_activation: bool,
    /// How long a cached substate may be served as the latest version of that substate. Raising it
    /// trades a wider window for handing out an already-spent input version against fewer validator
    /// round trips on hot substates. Requests for a specific version are unaffected.
    #[serde(default = "default_latest_substate_cache_ttl", with = "serializers::seconds")]
    pub latest_substate_cache_ttl: Duration,
    /// How many epochs past its terminal epoch a stored transaction is retained before it is pruned.
    /// A transaction's terminal epoch is the epoch it committed in once its receipt has been
    /// indexed, and its `max_epoch` — the last epoch it could still be sequenced in — until then, so
    /// a transaction that is never sequenced ages out on the same schedule as one that commits.
    /// Write `"forever"` to retain transactions indefinitely; `0` keeps only those that can still
    /// commit or committed in the current epoch.
    ///
    /// Applies to every stored transaction, whether submitted here or observed on the gossip topic.
    /// Only the transaction body and its locally recorded rejection reason are pruned; transaction
    /// receipts synced from the network are retained regardless, so a pruned transaction still
    /// resolves to its receipt-backed outcome. Set this well above the longest a client may take to
    /// poll for a result: once pruned, a transaction no longer appears in the recent-transactions
    /// listing or single transaction lookup, and a mempool rejection reason recorded for it is lost.
    /// Transactions stored before this indexer recorded a terminal epoch carry epoch 0, so the first
    /// pass on an indexer upgraded from a build that retained everything prunes that entire backlog.
    ///
    /// Pruning bounds database growth but does not return disk to the filesystem: SQLite reuses the
    /// freed pages rather than shrinking the file.
    #[serde(default = "default_transaction_retention_epochs", with = "retention_epochs")]
    pub transaction_retention_epochs: Option<u64>,
    /// Store transactions observed on the network-wide transaction gossip topic, not only those
    /// submitted directly to this indexer. When enabled the indexer joins the transaction mesh as a
    /// full participant: it validates what it receives and propagates it onward. Disabling it leaves
    /// the mesh entirely — no transaction is received, stored or forwarded.
    ///
    /// The gossip topic carries the whole network's transaction volume, so leaving this on while
    /// retaining forever grows the database without bound. Size retention against an adversary
    /// rather than against ordinary volume: validation deliberately omits the checks that depend on
    /// this node's view of runtime state, so correctly signed transactions that no validator will
    /// ever admit — an unknown template, a conflicting output — are still stored, and they are cheap
    /// to mint.
    #[serde(default = "default_index_gossiped_transactions")]
    pub index_gossiped_transactions: bool,
    /// Maximum total size of inbound transaction gossip awaiting storage. The gossip service drains
    /// this queue serially, so it absorbs bursts that arrive faster than validation and batched
    /// writes; once it is full, further messages are dropped rather than queued without limit. Sized
    /// in bytes because every message may be up to `gossip_sub_max_message_size`: at ordinary
    /// transaction sizes this admits a very deep backlog, while capping a flood of maximum-size
    /// messages.
    #[serde(default = "default_max_transaction_gossip_queue_bytes")]
    pub max_transaction_gossip_queue_bytes: usize,
    /// How long the transaction pruner idles between passes once it has nothing left to prune. While
    /// a backlog remains it drains in back-to-back batches rather than waiting out this interval.
    /// Only used when `transaction_retention_epochs` is set.
    #[serde(default = "default_transaction_prune_interval", with = "serializers::seconds")]
    pub transaction_prune_interval: Duration,
    /// The event filtering configuration
    pub event_filters: Vec<EventFilter>,
    /// Template addresses to watch for component creation/update events.
    /// Components created from these templates are tracked in a separate table for fast lookup.
    /// Defaults to the builtin liquidity pool template.
    #[serde(default = "default_watched_templates")]
    pub watched_templates: Vec<TemplateAddress>,
    /// When true (the default), substates fetched from validators to serve client reads must come
    /// with a proof that verifies against the shard group committee, or the read fails. Disabling
    /// trades verifiability for performance: values are served unverified, as fetched from a single
    /// (possibly byzantine or out-of-sync) validator.
    #[serde(default = "default_verify_substate_proofs")]
    pub verify_substate_proofs: bool,
    /// Rate-limiting configuration for the REST API endpoints
    pub rate_limits: IndexerRateLimitsConfig,
}

fn default_verify_substate_proofs() -> bool {
    true
}

fn default_latest_substate_cache_ttl() -> Duration {
    Duration::from_secs(2)
}

/// The subset of an indexer's configuration that is published over its API, as it affects what
/// clients see. Built once at startup: the API must expose exactly these values and nothing else
/// from `IndexerConfig`, which also holds local paths and listen addresses.
#[derive(Debug, Clone)]
pub struct PublishedIndexerConfig {
    pub sidechain_id: Option<RistrettoPublicKeyBytes>,
    pub transaction_retention_epochs: Option<u64>,
    pub index_gossiped_transactions: bool,
    pub verify_substate_proofs: bool,
    pub latest_substate_cache_ttl: Duration,
    pub indexes_all_events: bool,
}

impl From<&IndexerConfig> for PublishedIndexerConfig {
    fn from(config: &IndexerConfig) -> Self {
        Self {
            sidechain_id: config.sidechain_id.as_ref().map(|pk| pk.to_byte_type()),
            transaction_retention_epochs: config.transaction_retention_epochs,
            index_gossiped_transactions: config.index_gossiped_transactions,
            verify_substate_proofs: config.verify_substate_proofs,
            latest_substate_cache_ttl: config.latest_substate_cache_ttl,
            indexes_all_events: config.event_filters.is_empty() ||
                config.event_filters.iter().any(EventFilter::is_match_all),
        }
    }
}

fn default_transaction_retention_epochs() -> Option<u64> {
    Some(50)
}

mod retention_epochs {
    //! `transaction_retention_epochs` accepts a number of epochs or the string `"forever"`. An
    //! omitted key takes the default and TOML has no null literal, so without an explicit spelling
    //! for it, retaining indefinitely would be unreachable from a config file.

    use std::fmt;

    use serde::{
        Deserializer,
        Serializer,
        de::{self, Visitor},
    };

    const FOREVER: &str = "forever";

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where D: Deserializer<'de> {
        deserializer.deserialize_any(RetentionEpochsVisitor)
    }

    pub fn serialize<S>(value: &Option<u64>, s: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        match value {
            Some(epochs) => s.serialize_u64(*epochs),
            None => s.serialize_str(FOREVER),
        }
    }

    struct RetentionEpochsVisitor;

    impl<'de> Visitor<'de> for RetentionEpochsVisitor {
        type Value = Option<u64>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a number of epochs or the string \"{FOREVER}\"")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            u64::try_from(v)
                .map(Some)
                .map_err(|_| E::custom(format!("transaction_retention_epochs cannot be negative (got {v})")))
        }

        // The config layer hands every value through as a string, so a numeric literal in the file
        // arrives here rather than at `visit_u64`.
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.eq_ignore_ascii_case(FOREVER) {
                return Ok(None);
            }
            v.parse::<u64>().map(Some).map_err(|_| {
                E::custom(format!(
                    "expected a number of epochs or \"{FOREVER}\" for transaction_retention_epochs, got '{v}'"
                ))
            })
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_any(self)
        }
    }
}

fn default_index_gossiped_transactions() -> bool {
    true
}

fn default_max_transaction_gossip_queue_bytes() -> usize {
    128 * 1024 * 1024
}

fn default_transaction_prune_interval() -> Duration {
    Duration::from_secs(60 * 60)
}

fn default_watched_templates() -> Vec<TemplateAddress> {
    vec![tari_template_builtin::LIQUIDITY_POOL_TEMPLATE_ADDRESS]
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            override_from: None,
            identity_file: PathBuf::from("indexer_id.json"),
            data_dir: PathBuf::from("data/indexer"),
            p2p: P2pConfig::default(),
            api_listen_address: Some("127.0.0.1:18300".parse().unwrap()),
            metrics_listen_address: Some("127.0.0.1:18302".parse().unwrap()),
            graphql_address: Some("127.0.0.1:18301".parse().unwrap()),
            web_ui_address: Some("127.0.0.1:15000".parse().unwrap()),
            web_ui_public_api_url: None,
            web_ui_public_graphql_url: None,
            block_scanning_interval: Duration::from_secs(10),
            state_scanning_interval: Duration::from_secs(60),
            sidechain_id: None,
            dry_run_cache_ttl: Duration::from_secs(10),
            allow_past_protocol_activation: false,
            latest_substate_cache_ttl: default_latest_substate_cache_ttl(),
            transaction_retention_epochs: default_transaction_retention_epochs(),
            index_gossiped_transactions: default_index_gossiped_transactions(),
            max_transaction_gossip_queue_bytes: default_max_transaction_gossip_queue_bytes(),
            transaction_prune_interval: default_transaction_prune_interval(),
            event_filters: vec![],
            watched_templates: default_watched_templates(),
            verify_substate_proofs: default_verify_substate_proofs(),
            rate_limits: IndexerRateLimitsConfig::default(),
        }
    }
}

impl SubConfigPath for IndexerConfig {
    fn main_key_prefix() -> &'static str {
        "indexer"
    }
}

/// Rate-limiting configuration for the indexer REST API.
///
/// All `*_rate` values are **per IP address**. Each rate is a token-bucket
/// `(capacity, window)` pair: `capacity` is the burst size, and the bucket
/// refills at `capacity / window` tokens per second. A 10-second window keeps
/// the per-minute throughput intact while letting clients recover from a burst
/// in seconds rather than a full minute.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexerRateLimitsConfig {
    pub enabled: bool,
    /// POST /transactions – default 20 req / 10s burst (~120/min sustained)
    pub transactions_submit_rate: RefillRate,
    /// POST /transactions/dry-run – default 20 req / 10s burst (~120/min sustained)
    pub transactions_dry_run_submit_rate: RefillRate,
    /// POST /substates/fetch – default 10 req / 10s burst (~60/min sustained)
    pub substates_rate: RefillRate,
    /// POST /utxos/fetch – default 20 req / 10s burst (~120/min sustained)
    pub utxos_fetch_rate: RefillRate,
    /// GET /non-fungibles – default 10 req / 10s burst (~60/min sustained)
    pub non_fungibles_rate: RefillRate,
    /// GET /transactions/* read endpoints – default 5 req / 10s burst (~30/min sustained)
    pub transactions_rate: RefillRate,
    /// Maximum concurrent SSE connections per IP (default: 5)
    pub sse_max_connections_per_ip: usize,
    /// Trust X-Forwarded-For / X-Real-IP proxy headers (default: false).
    /// Only enable when the indexer is behind a trusted reverse proxy.
    pub trust_proxy_headers: bool,
}

impl Default for IndexerRateLimitsConfig {
    fn default() -> Self {
        let window = Duration::from_secs(10);
        Self {
            enabled: false,
            transactions_submit_rate: RefillRate::new(20.0, window).unwrap(),
            transactions_dry_run_submit_rate: RefillRate::new(20.0, window).unwrap(),
            substates_rate: RefillRate::new(20.0, Duration::from_secs(20)).unwrap(),
            utxos_fetch_rate: RefillRate::new(20.0, window).unwrap(),
            non_fungibles_rate: RefillRate::new(10.0, window).unwrap(),
            transactions_rate: RefillRate::new(5.0, window).unwrap(),
            sse_max_connections_per_ip: 5,
            trust_proxy_headers: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TOML has no null literal and an omitted key takes the default, so `"forever"` is the only
    /// way an operator can select unlimited retention. Losing it would strand anyone who relied on
    /// the old default with no way back to it.
    #[test]
    fn retention_epochs_accepts_forever_and_epoch_counts() {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            #[serde(default = "default_transaction_retention_epochs", with = "retention_epochs")]
            transaction_retention_epochs: Option<u64>,
        }

        let parse = |toml: &str| toml::from_str::<Wrapper>(toml).map(|w| w.transaction_retention_epochs);

        assert_eq!(parse("transaction_retention_epochs = 50").unwrap(), Some(50));
        assert_eq!(parse("transaction_retention_epochs = 0").unwrap(), Some(0));
        assert_eq!(parse(r#"transaction_retention_epochs = "forever""#).unwrap(), None);
        assert_eq!(parse(r#"transaction_retention_epochs = "FOREVER""#).unwrap(), None);
        // The config layer can hand a numeric literal through as a string.
        assert_eq!(parse(r#"transaction_retention_epochs = "50""#).unwrap(), Some(50));
        assert_eq!(parse("").unwrap(), default_transaction_retention_epochs());
        assert!(parse("transaction_retention_epochs = -1").is_err());
        assert!(parse(r#"transaction_retention_epochs = "never""#).is_err());
    }

    #[test]
    fn no_event_filters_indexes_every_event() {
        let config = IndexerConfig::default();
        assert!(config.event_filters.is_empty());
        assert!(PublishedIndexerConfig::from(&config).indexes_all_events);
    }

    /// The shipped config template declares one `[[indexer.event_filters]]` section with no fields,
    /// which matches every event. That must not read as a filtered indexer.
    #[test]
    fn an_empty_filter_indexes_every_event() {
        let config = IndexerConfig {
            event_filters: vec![EventFilter::default()],
            ..Default::default()
        };
        assert!(PublishedIndexerConfig::from(&config).indexes_all_events);
    }

    #[test]
    fn a_filter_that_narrows_events_is_reported_as_filtered() {
        let config = IndexerConfig {
            event_filters: vec![EventFilter {
                topic: Some("std.vault.deposit".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!PublishedIndexerConfig::from(&config).indexes_all_events);
    }

    /// A match-all filter alongside narrower ones still admits everything.
    #[test]
    fn a_match_all_filter_wins_over_narrower_ones() {
        let config = IndexerConfig {
            event_filters: vec![
                EventFilter {
                    topic: Some("std.vault.deposit".into()),
                    ..Default::default()
                },
                EventFilter::default(),
            ],
            ..Default::default()
        };
        assert!(PublishedIndexerConfig::from(&config).indexes_all_events);
    }
}
