//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use ootle_byte_type::ToByteType;
use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId};
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        KeysCreateRequest,
        KeysCreateResponse,
        KeysListRequest,
        KeysListResponse,
        KeysSetActiveRequest,
        KeysSetActiveResponse,
    },
};

use super::context::HandlerContext;

pub async fn handle_create(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: KeysCreateRequest,
) -> Result<KeysCreateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let key_manager = sdk.key_manager_api();
    let key = req
        .specific_index
        .map(|idx| key_manager.get_public_key(KeyId::derived(req.branch, idx)))
        .unwrap_or_else(|| key_manager.next_public_key(req.branch))?;
    Ok(KeysCreateResponse {
        id: key.key_id.derived_index().expect("Key is derived"),
        public_key: key.public_key.to_byte_type(),
    })
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: KeysListRequest,
) -> Result<KeysListResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::KeyList])?;
    let keys = sdk.key_manager_api().get_all_derived_keys(req.branch)?;
    Ok(KeysListResponse {
        keys: keys
            .into_iter()
            .map(|key| (key.key_id(), key.public_key().to_byte_type(), key.is_active()))
            .collect(),
    })
}

pub async fn handle_set_active(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: KeysSetActiveRequest,
) -> Result<KeysSetActiveResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let km = sdk.key_manager_api();
    km.set_active_key(KeyBranch::Account, req.index)?;
    let key = km.get_active_key(KeyBranch::Account)?;

    Ok(KeysSetActiveResponse {
        public_key: key.to_public_key().to_byte_type(),
    })
}
