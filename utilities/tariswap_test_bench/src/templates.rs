//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_indexer_client::{
    rest_api_client::IndexerRestApiClient,
    types::{ListTemplateCatalogueRequest, ListTemplateCatalogueResponse},
};
use tari_template_lib_types::TemplateAddress;

use crate::cli::CommonArgs;

pub async fn get_templates(cli: &CommonArgs) -> anyhow::Result<(TemplateAddress, TemplateAddress)> {
    let client = IndexerRestApiClient::connect(cli.indexer_url.clone())?;

    let tariswap = match cli.swap_template {
        Some(template_address) => template_address.as_template_address(),
        None => find_template(&client, "TariSwapPool").await?,
    };

    let faucet = match cli.faucet_template {
        Some(template_address) => template_address.as_template_address(),
        None => find_template(&client, "TestFaucet").await?,
    };

    log::info!("Faucet template: {}", faucet);
    log::info!("Tariswap template: {}", tariswap);

    Ok((faucet, tariswap))
}

async fn find_template(client: &IndexerRestApiClient, name: &str) -> anyhow::Result<TemplateAddress> {
    let ListTemplateCatalogueResponse { entries } = client
        .list_template_catalogue(ListTemplateCatalogueRequest {
            name_filter: Some(name.to_string()),
            limit: Some(100),
            after: None,
        })
        .await?;
    entries
        .into_iter()
        .find(|t| t.template_name.eq_ignore_ascii_case(name))
        .map(|t| t.template_address)
        .ok_or_else(|| anyhow::anyhow!("{name} template not found"))
}
