// Copyright 2023. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use minotari_app_utilities::common_cli_args::CommonCliArgs;
use tari_common::configuration::{ConfigOverrideProvider, Network as L1Network};
use tari_engine_types::substate::SubstateId;
use tari_ootle_app_utilities::{configuration::convert_l1_network_to_network, p2p_config::ReachabilityMode};
use tari_ootle_common_types::Network;
use url::Url;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Cli {
    #[clap(flatten)]
    pub common: CommonCliArgs,

    #[clap(long, short = 'a', multiple_values = true)]
    pub address: Vec<SubstateId>,

    #[clap(long, short = 'i')]
    pub scanning_interval: Option<u64>,

    /// Bind address for JSON-rpc server
    #[clap(long, short = 'r', alias = "rpc-address")]
    pub json_rpc_address: Option<SocketAddr>,
    #[clap(long, alias = "node-grpc", short = 'g', env = "TARI_INDEXER_MINOTARI_NODE_GRPC_URL")]
    pub epoch_oracle_minotari_node_grpc_url: Option<Url>,
    #[clap(long, alias = "oracle-config")]
    pub epoch_oracle_config: Option<PathBuf>,
    // Networking
    #[clap(long, short = 's')]
    pub peer_seeds: Vec<String>,
    /// Port to listen on for p2p connections
    #[clap(long)]
    pub listener_port: Option<u16>,
    /// Set the node to unreachable mode, the node will not act as a relay and will attempt relayed connections.
    #[clap(long)]
    pub reachability: Option<ReachabilityMode>,
    #[clap(long)]
    pub disable_mdns: bool,
    /// Public address of the JSON RPC
    #[clap(long, env = "TARI_INDEXER_WEB_UI_PUBLIC_JSON_RPC_URL")]
    pub web_ui_public_json_rpc_url: Option<String>,
    /// Public address of the GraphQL API
    #[clap(long, env = "TARI_INDEXER_WEB_UI_PUBLIC_GRAPHQL_URL")]
    pub web_ui_public_graphql_url: Option<String>,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }

    pub fn network_override(&self) -> Option<Network> {
        self.common.network.as_ref().map(convert_l1_network_to_network)
    }
}

impl ConfigOverrideProvider for Cli {
    fn get_config_property_overrides(&self, network: &L1Network) -> Vec<(String, String)> {
        let mut overrides = self.common.get_config_property_overrides(network);
        overrides.push(("network".to_string(), network.to_string()));
        overrides.push(("indexer.override_from".to_string(), network.to_string()));
        overrides.push(("p2p.seeds.override_from".to_string(), network.to_string()));

        if let Some(ref addr) = self.json_rpc_address {
            overrides.push(("indexer.json_rpc_address".to_string(), addr.to_string()));
        }

        if !self.peer_seeds.is_empty() {
            overrides.push(("p2p.seeds.peer_seeds".to_string(), self.peer_seeds.join(",")));
        }
        if let Some(listener_port) = self.listener_port {
            overrides.push(("indexer.p2p.listener_port".to_string(), listener_port.to_string()));
        }
        if let Some(seconds) = self.scanning_interval {
            overrides.push(("indexer.scanning_interval".to_string(), seconds.to_string()));
        }
        if let Some(ref json_rpc_url) = self.web_ui_public_json_rpc_url {
            overrides.push((
                "indexer.web_ui_public_json_rpc_url".to_string(),
                json_rpc_url.to_string(),
            ));
        }
        if let Some(ref graphql_url) = self.web_ui_public_graphql_url {
            overrides.push(("indexer.web_ui_public_graphql_url".to_string(), graphql_url.to_string()));
        }
        if let Some(reachability) = self.reachability {
            overrides.push(("indexer.p2p.reachability_mode".to_string(), reachability.to_string()));
        }
        if self.disable_mdns {
            overrides.push(("indexer.p2p.enable_mdns".to_string(), "false".to_string()));
        }
        if let Some(url) = self.epoch_oracle_minotari_node_grpc_url.as_ref() {
            overrides.push((
                "epoch_oracle.base_layer.base_node_grpc_url".to_string(),
                url.to_string(),
            ));
        }
        if let Some(ref config_path) = self.epoch_oracle_config {
            overrides.push((
                "epoch_oracle.configured.config_file".to_string(),
                config_path
                    .to_str()
                    .expect("epoch_oracle_config must be a UTF-8 string")
                    .to_string(),
            ));
        }
        overrides
    }
}
