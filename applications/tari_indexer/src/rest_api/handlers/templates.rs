//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    extract::{Path, Query},
    response::Response,
    Extension,
    Json,
};
use tari_indexer_client::types::{
    GetTemplateDefinitionResponse,
    ListTemplatesRequest,
    ListTemplatesResponse,
    TemplateMetadata,
};
use tari_ootle_common_types::optional::Optional;
use tari_ootle_storage::global::TemplateStatus;
use tari_template_lib_types::TemplateAddress;

use crate::{
    rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult},
    template_manager::TemplateManagerError,
};

#[utoipa::path(
    get,
    path = "/templates/{template_id}",
    description = "Fetch a template definition by its address"
)]
pub async fn get_template_definition(
    Extension(context): Extension<HandlerContext>,
    Path(template_address): Path<TemplateAddress>,
) -> HandlerResult<Response> {
    let template = context
        .template_manager()
        .fetch_and_load_template(&template_address)
        .optional()
        .map_err(|err| {
            // If it's pending, we return a 404 to the client - this allows them to retry later
            if matches!(err, TemplateManagerError::TemplateUnavailable {
                status: Some(TemplateStatus::Pending)
            }) {
                ErrorResponse::not_found(format!(
                    "Template with address {} is still being downloaded. Try again later.",
                    template_address
                ))
            } else {
                ErrorResponse::internal_error(format!("Error fetching template: {}", err))
            }
        })?
        .ok_or_else(|| ErrorResponse::not_found(format!("Template with address {} not found", template_address)))?;

    let resp = Json(GetTemplateDefinitionResponse {
        definition: template.template_def().clone(),
        name: template.template_name().to_string(),
        code_size: template.code_size(),
    });

    Ok(context.apply_cache_control(resp, 1000))
}

#[utoipa::path(
    get,
    path = "/templates/cached",
    description = "List all template cached by this indexer",
    params(
        ("limit" = Option<u32>, Query, description = "Limit the number of results returned"),
    ),
)]
pub async fn list_cached_templates(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListTemplatesRequest>,
) -> HandlerResult<Response> {
    let limit = req.limit.unwrap_or(10);
    if limit == 0 || limit > 100 {
        return Err(ErrorResponse::bad_request(
            "Limit must be between 1 and 100".to_string(),
        ));
    }

    let templates = context
        .template_manager()
        .fetch_template_metadata(limit as usize)
        .map_err(ErrorResponse::anyhow)?;

    let templates = templates
        .into_iter()
        .map(|t| TemplateMetadata {
            name: t.name,
            address: t.address,
            binary_sha: t.binary_sha,
            author_public_key: t.author_public_key,
            code_size: t.code_size,
            epoch: t.epoch,
        })
        .collect();

    let resp = Json(ListTemplatesResponse { templates });
    Ok(context.apply_cache_control(resp, 1000))
}
