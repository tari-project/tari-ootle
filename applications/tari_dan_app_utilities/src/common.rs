//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::{bail, Context};
use tari_base_node_client::BaseNodeClient;
use tari_common::configuration::Network;

pub async fn verify_correct_network<TClient: BaseNodeClient>(
    base_node_client: &mut TClient,
    configured_network: Network,
) -> anyhow::Result<()> {
    let base_node_network_byte = base_node_client.get_network().await?;

    let base_node_network =
        Network::try_from(base_node_network_byte).context("base node returned an invalid network byte")?;

    if configured_network != base_node_network {
        bail!(
            "Base node network is not the same as the configured network. Base node network: {}, Configured network: \
             {}.",
            base_node_network,
            configured_network,
        );
    }
    Ok(())
}
