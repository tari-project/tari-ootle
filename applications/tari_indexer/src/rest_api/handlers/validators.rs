//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::{Extension, Json, extract::Query, response::Response};
use tari_epoch_manager::EpochManagerReader;
use tari_indexer_client::types::{ListValidatorsRequest, ListValidatorsResponse, ValidatorInfo};

use crate::rest_api::{context::HandlerContext, error::ErrorResponse, handlers::HandlerResult};

#[utoipa::path(
    get,
    path = "/validators",
    description = "Get the validator roster for an epoch as tracked by the epoch manager",
    params(
        ("epoch" = Option<u64>, Query, description = "Epoch to fetch the roster for. Defaults to the current epoch"),
    ),
    responses(
        (status = 200, description = "The validator roster for the epoch", body = ListValidatorsResponse),
        (status = BAD_REQUEST, description = "The requested epoch has not occurred yet", body = ErrorResponse),
        (status = INTERNAL_SERVER_ERROR, description = "Internal error", body = ErrorResponse),
    )
)]
pub async fn list_validators(
    Extension(context): Extension<HandlerContext>,
    Query(req): Query<ListValidatorsRequest>,
) -> HandlerResult<Response> {
    let current_epoch = context.epoch_manager().get_current_epoch();
    let epoch = req.epoch.unwrap_or(current_epoch);
    if epoch > current_epoch {
        return Err(ErrorResponse::bad_request(format!(
            "Epoch {epoch} has not occurred yet (current epoch is {current_epoch})"
        )));
    }

    let num_preshards = context
        .epoch_manager()
        .get_network_description()
        .await
        .map_err(ErrorResponse::anyhow)?
        .num_preshards;
    let num_committees = context
        .epoch_manager()
        .get_num_committees(epoch)
        .await
        .map_err(ErrorResponse::anyhow)?;

    let validators = context
        .epoch_manager()
        .get_all_validator_nodes(epoch)
        .await
        .map_err(ErrorResponse::anyhow)?
        .into_iter()
        .map(|vn| ValidatorInfo {
            public_key: vn.public_key,
            peer_id: vn.address.to_string(),
            shard_group: vn.shard_key.to_shard_group(num_preshards, num_committees),
            start_epoch: vn.start_epoch,
            end_epoch: vn.end_epoch,
            fee_claim_public_key: vn.fee_claim_public_key,
            vote_power: vn.vote_power.value(),
        })
        .collect();

    Ok(context.apply_cache_control(Json(ListValidatorsResponse { epoch, validators }), 30))
}
