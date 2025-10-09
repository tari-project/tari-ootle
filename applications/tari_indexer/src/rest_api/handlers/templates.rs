//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{
    extract::{Path, Query},
    response::Response,
    Extension,
    Json,
};
use tari_engine::{template::TemplateModuleLoader, wasm::WasmModule};
use tari_indexer_client::types::{
    GetTemplateDefinitionResponse,
    ListTemplatesRequest,
    ListTemplatesResponse,
    TemplateMetadata,
};
use tari_ootle_common_types::optional::Optional;
use tari_ootle_storage::global::TemplateStatus;
use tari_template_lib::types::TemplateAddress;
use tari_template_manager::interface::{TemplateExecutable, TemplateManagerError};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

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
        .fetch_template(&template_address)
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

    let template = match template.executable {
        TemplateExecutable::CompiledWasm(code) => WasmModule::from_code(code)
            .load_template()
            .map_err(|e| ErrorResponse::internal_error(format!("Error loading template: {}", e)))?,
        // TemplateExecutable::DownloadableWasm is never returned ad there is no DB type for that
        TemplateExecutable::DownloadableWasm(_, _) | TemplateExecutable::Manifest(_) => {
            return Err(ErrorResponse::bad_request("Template is not a wasm module".to_string()));
        },
    };
    let resp = Json(GetTemplateDefinitionResponse {
        definition: template.template_def().clone(),
        name: template.template_name().to_string(),
    });

    Ok(context.apply_cache_control(resp, 1000))
}

#[utoipa::path(
    get,
    path = "/templates",
    description = "List all templates",
    params(
        ("limit" = Option<u32>, Query, description = "Limit the number of results returned"),
    ),
)]
pub async fn list_templates(
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
    let resp = Json(ListTemplatesResponse {
        templates: templates
            .into_iter()
            .map(|t| TemplateMetadata {
                name: t.name,
                address: t.address,
                binary_sha: t.binary_sha,
            })
            .collect(),
    });

    Ok(context.apply_cache_control(resp, 1000))
}
