//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::apis::config::ConfigKey;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{NetworkInfo, SettingsGetResponse, SettingsSetRequest, SettingsSetResponse},
};

use crate::handlers::HandlerContext;

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _value: serde_json::Value,
) -> Result<SettingsGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let indexer_url = sdk
        .config_api()
        .get(ConfigKey::IndexerUrl)
        .optional()?
        .unwrap_or_else(|| sdk.get_network_interface().get_endpoint().to_string());
    let network = sdk.config_api().get_network()?;

    Ok(SettingsGetResponse {
        indexer_url,
        network: NetworkInfo {
            name: network.to_string(),
            byte: network.as_byte(),
        },
    })
}

pub async fn handle_set(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SettingsSetRequest,
) -> Result<SettingsSetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    sdk.get_network_interface().set_endpoint(&req.indexer_url)?;
    sdk.config_api().set(ConfigKey::IndexerUrl, &req.indexer_url)?;
    Ok(SettingsSetResponse {})
}
