//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{Extension, Json, extract::Query};
use tari_indexer_client::types::{
    ListWatchedSubstatesRequest,
    ListWatchedSubstatesResponse,
    ListWatchedTemplatesResponse,
    WatchedSubstateItem,
    WatchedTemplateItem,
};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/substates/watched",
    description = "Lists component addresses created from watched templates",
    params(
        ("template_address" = Option<String>, Query, description = "Filter by template address"),
        ("limit" = Option<u64>, Query, description = "Maximum number of results (default: 50, max: 200)"),
        ("offset" = Option<u64>, Query, description = "Offset for pagination (default: 0)"),
    ),
    responses(
        (status = 200, description = "List of watched component addresses", body = ListWatchedSubstatesResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Failed to list watched substates", body = ErrorResponse),
    )
)]
pub async fn list_watched_substates(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListWatchedSubstatesRequest>,
) -> HandlerResult<Json<ListWatchedSubstatesResponse>> {
    let limit = req.limit.unwrap_or(50).min(200);
    let offset = req.offset.unwrap_or(0);

    let entries = context
        .read_only_store()
        .list_watched_substates(req.template_address.as_ref(), limit, offset)
        .await
        .map_err(ErrorResponse::anyhow)?;

    let substates = entries
        .into_iter()
        .map(|e| WatchedSubstateItem {
            component_address: e.component_address,
            template_address: e.template_address,
        })
        .collect();

    Ok(Json(ListWatchedSubstatesResponse { substates }))
}

#[utoipa::path(
    get,
    path = "/templates/watched",
    description = "Lists template addresses that the indexer is watching for component creation events",
    responses(
        (status = 200, description = "List of watched template addresses", body = ListWatchedTemplatesResponse),
    )
)]
pub async fn list_watched_templates(
    Extension(context): Extension<HandlerContext>,
) -> HandlerResult<Json<ListWatchedTemplatesResponse>> {
    let store = context.read_only_store();
    let mut templates = Vec::new();
    for addr in context.watched_templates().iter() {
        let template_name = store
            .get_template_catalogue_entry(addr)
            .await
            .ok()
            .flatten()
            .map(|e| e.template_name);
        templates.push(WatchedTemplateItem {
            template_address: *addr,
            template_name,
        });
    }
    Ok(Json(ListWatchedTemplatesResponse { templates }))
}
