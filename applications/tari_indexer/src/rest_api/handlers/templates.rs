//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    Extension,
    Json,
    extract::{Path, Query},
    response::Response,
};
use tari_indexer_client::types::{
    GetTemplateDefinitionResponse,
    ListTemplateCatalogueRequest,
    ListTemplateCatalogueResponse,
    TemplateCatalogueItem,
};
use tari_ootle_common_types::{optional::Optional, services::template_provider::TemplateProvider};
use tari_template_lib_types::TemplateAddress;
use tokio::task;

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/templates/{template_address}",
    description = "Fetch a template definition by its address",
    responses(
        (status = 200, description = "Template definition", body = GetTemplateDefinitionResponse),
        (status = 404, description = "Template not found", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch template definition", body = ErrorResponse),
    ),
)]
pub async fn get_template_definition(
    Extension(context): Extension<HandlerContext>,
    Path(template_address): Path<TemplateAddress>,
) -> HandlerResult<Response> {
    let template_provider = context.dry_run_transaction_processor().template_provider().clone();
    let template = task::spawn_blocking(move || template_provider.get_template(&template_address))
        .await
        .map_err(ErrorResponse::anyhow)?
        .map_err(|err| ErrorResponse::internal_error(format!("Error fetching template: {}", err)))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Template with address {} not found", template_address)))?;

    let resp = Json(GetTemplateDefinitionResponse {
        definition: template.template_def().clone(),
        name: template.template_name().to_string(),
        code_size: template.code_size(),
    });

    Ok(context.apply_cache_control(resp, 120 * 60))
}

#[utoipa::path(
    get,
    path = "/templates/catalogue",
    description = "List templates discovered on the network via the template catalogue",
    params(
        ("name_filter" = Option<String>, Query, description = "Substring filter on template name"),
        ("limit" = Option<u64>, Query, description = "Maximum entries to return (default: 20, max: 100)"),
        ("after" = Option<String>, Query, description = "Cursor: return entries inserted after the row with this template address. Omit to start from the beginning"),
    ),
    responses(
        (status = 200, description = "Template catalogue entries", body = ListTemplateCatalogueResponse),
        (status = BAD_REQUEST, description = "Invalid request parameters", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to list template catalogue", body = ErrorResponse),
    ),
)]
pub async fn list_template_catalogue(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListTemplateCatalogueRequest>,
) -> HandlerResult<Response> {
    let limit = req.limit.unwrap_or(20);
    if limit == 0 || limit > 100 {
        return Err(ErrorResponse::bad_request(
            "Limit must be between 1 and 100".to_string(),
        ));
    }

    let entries = context
        .read_only_store()
        .list_template_catalogue(req.name_filter.as_deref(), req.after.as_ref(), limit)
        .await
        .map_err(ErrorResponse::anyhow)?
        .into_iter()
        .map(TemplateCatalogueItem::from)
        .collect();

    Ok(context.apply_cache_control(Json(ListTemplateCatalogueResponse { entries }), 30))
}

#[utoipa::path(
    get,
    path = "/templates/catalogue/{template_address}",
    description = "Get a single template catalogue entry by its address",
    responses(
        (status = 200, description = "Template catalogue entry", body = TemplateCatalogueItem),
        (status = 404, description = "Template not found in catalogue", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to fetch template catalogue entry", body = ErrorResponse),
    ),
)]
pub async fn get_template_catalogue_entry(
    Extension(context): Extension<HandlerContext>,
    Path(template_address): Path<TemplateAddress>,
) -> HandlerResult<Response> {
    let entry = context
        .read_only_store()
        .get_template_catalogue_entry(&template_address)
        .await
        .optional()
        .map_err(ErrorResponse::anyhow)?
        .ok_or_else(|| ErrorResponse::not_found(format!("Template {} not found in the catalogue", template_address)))?;

    Ok(context.apply_cache_control(Json(TemplateCatalogueItem::from(entry)), 120 * 60))
}
