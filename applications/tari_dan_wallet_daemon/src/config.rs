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

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use config::Config;
use serde::{Deserialize, Serialize};
use tari_common::{
    configuration::{CommonConfig, Network},
    ConfigurationError,
    DefaultConfigLoader,
    SubConfigPath,
};
use tari_crypto::tari_utilities::SafePassword;
use tari_dan_common_types::crypto::create_secret_password;
use url::Url;

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub common: CommonConfig,
    pub dan_wallet_daemon: WalletDaemonConfig,
}

impl ApplicationConfig {
    pub fn load_from(cfg: &Config) -> Result<Self, ConfigurationError> {
        let config = Self {
            common: CommonConfig::load_from(cfg)?,
            dan_wallet_daemon: WalletDaemonConfig::load_from(cfg)?,
        };
        Ok(config)
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
    pub json_rpc_address: Option<SocketAddr>,
    /// The public URL to JSON-RPC server used by the web UI. If not specified, the json_rpc_address is used.
    pub web_ui_public_json_rpc_url: Option<String>,
    /// The signaling server address for the webrtc
    pub signaling_server_address: Option<SocketAddr>,
    /// The indexer JSON-RPC url
    pub indexer_json_rpc_url: Url,
    /// Expiration duration of the JWT token
    #[serde(with = "humantime_serde")]
    #[serde(default = "return_default_jwt_expiry")]
    pub jwt_expiry: Duration,
    /// Secret key for the JWT token.
    #[serde(default = "create_secret_password")]
    pub jwt_secret_key: SafePassword,
    /// The address of the Web UI
    pub web_ui_address: Option<SocketAddr>,
    /// The path to the value lookup table binary file used for brute force value lookups. This setting
    /// is only used when attempting to view confidential balances in confidential resources that use a view key
    /// controlled by this wallet. The binary file can be generated using the generate_ristretto_value_lookup
    /// utility. If this is not set, the value lookup table will be generated on the fly which will have a large
    /// performance cost when brute forcing high-value outputs.
    pub value_lookup_table_file: Option<PathBuf>,
}

fn return_default_jwt_expiry() -> Duration {
    // TODO: Come up with a reasonable default value
    Duration::from_secs(500 * 60)
}

impl Default for WalletDaemonConfig {
    fn default() -> Self {
        Self {
            override_from: None,
            network: Network::Igor,
            authentication: WalletDaemonAuth::default(),
            json_rpc_address: Some(SocketAddr::from(([127u8, 0, 0, 1], 9000))),
            web_ui_public_json_rpc_url: None,
            signaling_server_address: Some(SocketAddr::from(([127u8, 0, 0, 1], 9100))),
            indexer_json_rpc_url: "http://127.0.0.1:18300/json_rpc"
                .parse()
                .expect("failed to parse default indexer_json_rpc_url"),
            jwt_expiry: return_default_jwt_expiry(),
            jwt_secret_key: create_secret_password(),
            web_ui_address: Some("127.0.0.1:5100".parse().unwrap()),
            value_lookup_table_file: None,
        }
    }
}

impl SubConfigPath for WalletDaemonConfig {
    fn main_key_prefix() -> &'static str {
        "dan_wallet_daemon"
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum WalletDaemonAuth {
    #[default]
    None,
    WebAuthn,
}
