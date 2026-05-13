//  Copyright 2022. The Tari Project
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

use std::{
    fmt::{Display, Formatter},
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use anyhow::anyhow;
use config::Config;
use serde::{Deserialize, Serialize};
use tari_common::{ConfigurationError, DefaultConfigLoader, SubConfigPath, configuration::CommonConfig};
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_address::Network;
use url::Url;

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub common: CommonConfig,
    pub ootle_wallet_daemon: WalletDaemonConfig,
}

impl ApplicationConfig {
    pub fn load_from(cfg: &Config) -> Result<Self, ConfigurationError> {
        let config = Self {
            common: CommonConfig::load_from(cfg)?,
            ootle_wallet_daemon: WalletDaemonConfig::load_from(cfg)?,
        };
        Ok(config)
    }

    pub fn to_data_dir(&self) -> PathBuf {
        self.common.base_path.join("data")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct WalletDaemonConfig {
    override_from: Option<String>,
    /// Network that is used to send transactions.
    pub network: Network,
    /// Authentication method.
    pub authentication: WalletDaemonAuth,
    /// The wallet daemon listening address
    #[serde(default = "return_default_rpc_address")]
    pub json_rpc_address: SocketAddr,
    /// The signaling server address for the webrtc
    pub signaling_server_address: Option<SocketAddr>,
    /// The indexer API url
    pub indexer_api_url: Url,
    /// Expiration duration of the JWT token
    #[serde(with = "humantime_serde")]
    #[serde(default = "return_default_jwt_expiry")]
    pub jwt_expiry: Duration,
    /// If true, the wallet daemon will allow CORS requests from any origin. This is useful for development and
    /// testing, but should be disabled in production.
    pub enable_permissive_cors: bool,
    pub webauthn: WebAuthnConfig,
    /// The path to the value lookup table binary file used for brute force value lookups. This setting
    /// is only used when attempting to view confidential balances in confidential resources that use a view key
    /// controlled by this wallet. The binary file can be generated using the generate_ristretto_value_lookup
    /// utility. If this is not set, the value lookup table will be generated on the fly which will have a large
    /// performance cost when brute forcing high-value outputs.
    pub value_lookup_table_file: Option<PathBuf>,
    /// The number of contiguous failures to find an account derived from a public key before abandoning recovery and
    /// assuming that there are no further accounts.
    pub recovery_abandon_count: usize,
    /// Directory where burn proof files are read from after a burn transaction completes.
    /// If not set, defaults to the platform data directory (e.g. ~/.local/share/tari/{network}/burn_proofs on Linux,
    /// ~/Library/Application Support/tari/{network}/burn_proofs on macOS).
    pub burn_proof_dir: Option<PathBuf>,
    pub override_keyring_password: Option<SafePassword>,
    /// If true (default), the wallet daemon will watch the burn_proof_dir and automatically submit claim
    /// transactions when new burn proof files are detected, deferring each claim to the next epoch as required.
    #[serde(default = "return_default_auto_claim_burns")]
    pub auto_claim_burns: bool,
}

impl WalletDaemonConfig {
    pub fn get_burn_proof_dir(&self, network: Network) -> PathBuf {
        self.burn_proof_dir
            .clone()
            .unwrap_or_else(|| default_burn_proof_dir(network))
    }
}

fn return_default_auto_claim_burns() -> bool {
    true
}

fn return_default_jwt_expiry() -> Duration {
    // Suggested expiry for access tokens is between 5min and 1h
    Duration::from_secs(5 * 60)
}

fn return_default_rpc_address() -> SocketAddr {
    SocketAddr::from(([127u8, 0, 0, 1], 5100))
}

impl Default for WalletDaemonConfig {
    fn default() -> Self {
        Self {
            override_from: None,
            network: Network::default(),
            authentication: WalletDaemonAuth::default(),
            json_rpc_address: return_default_rpc_address(),
            signaling_server_address: Some(SocketAddr::from(([127u8, 0, 0, 1], 9100))),
            indexer_api_url: "http://127.0.0.1:18300"
                .parse()
                .expect("failed to parse default indexer_api_url"),
            jwt_expiry: return_default_jwt_expiry(),
            enable_permissive_cors: false,
            value_lookup_table_file: None,
            recovery_abandon_count: 10,
            webauthn: WebAuthnConfig {
                rp_origin: None,
                rp_id: "localhost".to_string(),
                session_ttl: Duration::from_secs(60 * 60),
            },
            burn_proof_dir: None,
            override_keyring_password: None,
            auto_claim_burns: true,
        }
    }
}

fn default_burn_proof_dir(network: Network) -> PathBuf {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tari")
        .join(network.as_key_str())
        .join("burn_proofs")
}

impl SubConfigPath for WalletDaemonConfig {
    fn main_key_prefix() -> &'static str {
        "ootle_wallet_daemon"
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct WebAuthnConfig {
    /// if not set, rp_origin will be set to http://localhost:{web_ui_address.port()}
    pub rp_origin: Option<Url>,
    pub rp_id: String,
    #[serde(with = "humantime_serde")]
    pub session_ttl: Duration,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum WalletDaemonAuth {
    #[default]
    None,
    WebAuthn,
    ApiKey,
}

impl Display for WalletDaemonAuth {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletDaemonAuth::None => write!(f, "none"),
            WalletDaemonAuth::WebAuthn => write!(f, "webauthn"),
            WalletDaemonAuth::ApiKey => write!(f, "api_key"),
        }
    }
}

impl FromStr for WalletDaemonAuth {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(WalletDaemonAuth::None),
            "webauthn" => Ok(WalletDaemonAuth::WebAuthn),
            "api_key" | "apikey" => Ok(WalletDaemonAuth::ApiKey),
            _ => Err(anyhow!("Invalid authentication method: {}", s)),
        }
    }
}
