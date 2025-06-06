//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::headers::authorization::Bearer;
use tari_ootle_wallet_sdk::network::WalletNetworkInterface;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        AuthoredTemplate,
        TemplatesGetRequest,
        TemplatesGetResponse,
        TemplatesListAuthoredRequest,
        TemplatesListAuthoredResponse,
    },
};

use crate::handlers::HandlerContext;

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TemplatesGetRequest,
) -> Result<TemplatesGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::TemplatesRead])?;

    let template_definition = sdk
        .get_network_interface()
        .fetch_template_definition(req.template_address)
        .await?;

    Ok(TemplatesGetResponse { template_definition })
}

pub async fn handle_list_owned(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TemplatesListAuthoredRequest,
) -> Result<TemplatesListAuthoredResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TemplatesRead])?;

    let (templates, total_templates) =
        context
            .wallet_sdk()
            .template_api()
            .list_authored_templates(&req.author_public_key, req.page, req.page_size)?;
    Ok(TemplatesListAuthoredResponse {
        templates: templates.iter().map(AuthoredTemplate::from).collect(),
        total_templates,
    })
}
