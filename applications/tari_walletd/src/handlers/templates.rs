//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::network::WalletNetworkInterface;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        AuthoredTemplate,
        TemplatesGetRequest,
        TemplatesGetResponse,
        TemplatesListAuthoredRequest,
        TemplatesListAuthoredResponse,
    },
};

use crate::handlers::{HandlerContext, helpers::not_found};

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TemplatesGetRequest,
) -> Result<TemplatesGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::TemplatesRead])?;

    if let Some(template) = sdk
        .template_api()
        .fetch_authored_template(req.template_address)
        .optional()?
    {
        return Ok(TemplatesGetResponse {
            template_definition: template.into(),
        });
    }

    let template_definition = sdk
        .get_network_interface()
        .fetch_template_definition(req.template_address)
        .await
        .optional()?
        .ok_or_else(|| not_found(format!("Template not found at address {}", req.template_address)))?;

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
            .list_templates(req.author_public_key.as_ref(), req.page, req.page_size)?;
    Ok(TemplatesListAuthoredResponse {
        templates: templates.into_iter().map(AuthoredTemplate::from).collect(),
        total_templates,
    })
}
