//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_indexer_client::types::{ListTemplatesRequest, ListTemplatesResponse};
use tari_template_lib::types::TemplateAddress;

use crate::cli::CommonArgs;

pub async fn get_templates(cli: &CommonArgs) -> anyhow::Result<(TemplateAddress, TemplateAddress)> {
    let mut client = tari_indexer_client::rest_api_client::IndexerRestApiClient::connect(cli.indexer_url.clone())?;

    let templates = if cli.swap_template.is_none() || cli.faucet_template.is_none() {
        let ListTemplatesResponse { templates } = client
            .list_cached_templates(ListTemplatesRequest { limit: Some(100) })
            .await?;
        templates
    } else {
        vec![]
    };

    let tariswap = if let Some(template_address) = cli.swap_template {
        template_address
    } else {
        templates
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case("TariSwapPool"))
            .map(|t| t.address)
            .ok_or(anyhow::anyhow!("Tariswap template not found"))?
    };

    let faucet = if let Some(template_address) = cli.faucet_template {
        template_address
    } else {
        templates
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case("TestFaucet"))
            .map(|t| t.address)
            .ok_or(anyhow::anyhow!("Faucet template not found"))?
    };

    log::info!("Faucet template: {}", faucet);
    log::info!("Tariswap template: {}", tariswap);

    Ok((faucet, tariswap))
}
