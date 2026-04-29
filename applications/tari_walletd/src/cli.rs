//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser};
use tari_common::configuration::{ConfigOverrideProvider, Network as L1Network};
use tari_common_types::seeds::seed_words::SeedWords;
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_address::Network;
use tari_ootle_app_utilities::common_cli_args::CommonCliArgs;
use url::Url;

use crate::config::WalletDaemonAuth;

#[derive(Args, Debug)]
pub struct WalletRestoreArgs {
    /// Seed words of a wallet to be restored.
    /// If set, wallet daemon tries to restore your wallet based on these seed words.
    #[clap(long)]
    pub seed_words: Option<SeedWords>,
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Cli {
    #[clap(flatten)]
    pub common: CommonCliArgs,
    #[clap(long, short = 'l', alias = "listen-on", env = "JRPC_LISTEN_ON")]
    /// The address to listen on for Web and JSON-RPC requests. Defaults to localhost:5100.
    pub listen_on: Option<SocketAddr>,
    #[clap(long, env = "SIGNALING_SERVER_ADDRESS")]
    pub signaling_server_address: Option<SocketAddr>,
    #[clap(long, short = 'i', alias = "indexer-url")]
    /// Indexer API url override
    pub indexer_api_url: Option<Url>,
    #[clap(flatten)]
    pub wallet_restore: WalletRestoreArgs,
    /// The OS keyring is used to store and retrieve a randomly generated password. This is used for wallet encryption.
    /// This setting overrides this functionality, using this password instead of generating and storing one.
    /// This is useful if a keyring is not available on your platform or if there is some other preference to use a
    /// specific password.
    /// NOTE: Once this is set, it must always be set to access the wallet.
    #[clap(long)]
    pub override_keyring_password: Option<SafePassword>,
    /// The authentication method to use for the wallet daemon. This controls how clients must authenticate when making
    /// requests to the wallet daemon.
    #[clap(long, alias = "auth")]
    pub authentication: Option<WalletDaemonAuth>,
    /// The path to the value lookup table binary file used for brute force value lookups. This setting
    /// is only used when attempting to view confidential balances in confidential resources that use a view key
    /// controlled by this wallet. The binary file can be generated using the generate_ristretto_value_lookup
    /// utility. If this is not set, the value lookup table will be generated on the fly which will have a large
    /// performance cost when brute forcing high-value outputs.
    #[clap(long, alias = "lookup-file")]
    pub value_lookup_table_file: Option<PathBuf>,
    /// If set, the wallet daemon will enable permissive CORS, and set webauthn.rp_origin to http://localhost:{enable_vite_dev_port}
    /// to allow authentication from a local development server (e.g. npm run dev).
    /// This is useful for development and testing, but should not be used in production.
    #[clap(long, alias = "vite-dev")]
    pub enable_vite_dev_port: Option<u16>,
    #[clap(subcommand)]
    pub command: Option<Subcommand>,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }

    pub fn network(&self) -> Network {
        self.common.network
    }
}

impl ConfigOverrideProvider for Cli {
    fn get_config_property_overrides(&self, network: &L1Network) -> Vec<(String, String)> {
        let mut overrides = self.common.get_config_property_overrides(network);
        overrides.push(("ootle_wallet_daemon.override_from".to_string(), network.to_string()));
        if let Some(json_rpc_address) = self.listen_on {
            overrides.push((
                format!("{}.ootle_wallet_daemon.json_rpc_address", network),
                json_rpc_address.to_string(),
            ));
        }
        if let Some(ref signaling_server_address) = self.signaling_server_address {
            overrides.push((
                format!("{}.ootle_wallet_daemon.signaling_server_address", network),
                signaling_server_address.to_string(),
            ));
        }
        if let Some(ref indexer_api_url) = self.indexer_api_url {
            overrides.push((
                format!("{}.ootle_wallet_daemon.indexer_api_url", network),
                indexer_api_url.to_string(),
            ));
        }
        if let Some(ref file) = self.value_lookup_table_file {
            overrides.push((
                format!("{}.ootle_wallet_daemon.value_lookup_table_file", network),
                file.display().to_string(),
            ));
        }
        if let Some(ref auth) = self.authentication {
            overrides.push((
                format!("{}.ootle_wallet_daemon.authentication", network),
                auth.to_string(),
            ));
        }
        if let Some(port) = self.enable_vite_dev_port {
            overrides.extend([
                (
                    format!("{}.ootle_wallet_daemon.enable_permissive_cors", network),
                    "true".to_string(),
                ),
                (
                    format!("{}.ootle_wallet_daemon.webauthn.rp_origin", network),
                    format!("http://localhost:{port}"),
                ),
            ]);
        }
        overrides
    }
}

#[derive(clap::Subcommand, Debug)]
pub enum Subcommand {
    #[clap(name = "run", about = "Run the wallet daemon")]
    Run,
    #[clap(about = "Generate a new key and output the public key")]
    CreateAccount {
        #[clap(long)]
        name: Option<String>,
        #[clap(long, alias = "key")]
        key_index: Option<u64>,
        #[clap(long)]
        set_active: bool,
        #[clap(long, alias = "output", short = 'o')]
        output_path: Option<PathBuf>,
    },
    #[clap(about = "Generate a key to use for resources with viewable balances")]
    NewViewableBalanceKey {
        #[clap(long, alias = "key")]
        key_index: u64,
        #[clap(long, alias = "output", short = 'o')]
        output_path: Option<PathBuf>,
    },
    #[clap(
        name = "seed-words",
        about = "Get current seed words of wallet (used for wallet retrieval)"
    )]
    SeedWords,
    #[clap(
        name = "reset",
        about = "Reset the wallet by deleting all on-chain state. Intended for testnet resets only. The seed key is \
                 preserved in the OS keyring."
    )]
    Reset {
        /// Skip the interactive confirmation prompt
        #[clap(long)]
        confirm: bool,
    },
}
