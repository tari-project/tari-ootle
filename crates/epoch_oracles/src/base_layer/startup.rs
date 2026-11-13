//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Write};

use log::*;
use ootle_network::Network;
use tari_base_node_client::BaseNodeClient;

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::base_layer::startup";

/// Checks the base nodes an oracle is about to scan from. At least one must be reachable, and every
/// reachable one must be on `network`. An unreachable base node passes: the oracle fails over to it
/// once it is up, and checks its network before using it.
pub async fn check_base_nodes_at_startup<TClient: BaseNodeClient + Display>(
    base_nodes: &mut [TClient],
    network: Network,
) -> Result<(), BaseNodeStartupError> {
    let mut num_reachable = 0usize;
    let mut unreachable = String::new();
    for base_node in base_nodes.iter_mut() {
        match base_node.get_network().await {
            Ok(network_byte) => {
                let actual = Network::try_from(network_byte).map_err(|_| BaseNodeStartupError::InvalidNetwork {
                    base_node: base_node.to_string(),
                    network_byte,
                })?;
                if actual != network {
                    return Err(BaseNodeStartupError::WrongNetwork {
                        base_node: base_node.to_string(),
                        expected: network,
                        actual,
                    });
                }
                info!(target: LOG_TARGET, "Base node {base_node} is reachable on {network}");
                num_reachable += 1;
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "Base node {base_node} is not reachable: {err}");
                let separator = if unreachable.is_empty() { "" } else { "; " };
                let _ignore = write!(unreachable, "{separator}{base_node}: {err}");
            },
        }
    }
    if num_reachable == 0 {
        return Err(BaseNodeStartupError::NoneReachable { details: unreachable });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum BaseNodeStartupError {
    #[error(
        "Could not connect to any configured Minotari node ({details}). Please ensure that a Minotari node is running \
         and configured for GRPC."
    )]
    NoneReachable { details: String },
    #[error("Minotari node {base_node} reported invalid network byte {network_byte}")]
    InvalidNetwork { base_node: String, network_byte: u8 },
    #[error("Minotari node {base_node} is on {actual}, but this node is configured for {expected}")]
    WrongNetwork {
        base_node: String,
        expected: Network,
        actual: Network,
    },
}
