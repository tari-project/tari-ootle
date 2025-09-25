// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{StealthUtxosListRequest, StealthUtxosListResponse, UtxoInfo},
};

use crate::handlers::HandlerContext;

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosListRequest,
) -> Result<StealthUtxosListResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::AccountList(None)])?;

    let utxos = context.wallet_sdk().stealth_outputs_api().utxos_get_many(
        &req.resource_address,
        req.account_address.as_ref(),
        req.filter_by_status,
    )?;

    Ok(StealthUtxosListResponse {
        utxos: utxos
            .into_iter()
            .map(|o| UtxoInfo {
                address: o.to_utxo_address(),
                value: o.value,
                status: o.status,
                is_burnt: o.is_burnt,
                is_frozen: o.is_frozen,
                is_on_chain: o.is_on_chain,
            })
            .collect(),
    })
}
