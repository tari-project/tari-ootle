//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        AddressBookAddRequest,
        AddressBookAddResponse,
        AddressBookDeleteRequest,
        AddressBookDeleteResponse,
        AddressBookGetRequest,
        AddressBookGetResponse,
        AddressBookListRequest,
        AddressBookListResponse,
        AddressBookUpdateRequest,
        AddressBookUpdateResponse,
    },
};

use crate::handlers::HandlerContext;

fn validate_address(address: &str) -> Result<(), anyhow::Error> {
    if !address.starts_with("otl_loc_") {
        return Err(anyhow!("Invalid address format: must be a valid Ootle address (otl_loc_...)"));
    }
    Ok(())
}

pub async fn handle_add(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookAddRequest,
) -> Result<AddressBookAddResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    validate_address(&req.address)?;

    let entry = sdk
        .address_book_api()
        .add(&req.name, &req.address, req.memo.as_deref())?;

    Ok(AddressBookAddResponse { entry })
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _req: AddressBookListRequest,
) -> Result<AddressBookListResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let entries = sdk.address_book_api().list()?;

    Ok(AddressBookListResponse { entries })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookGetRequest,
) -> Result<AddressBookGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let entry = sdk.address_book_api().get(&req.name)?;

    Ok(AddressBookGetResponse { entry })
}

pub async fn handle_update(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookUpdateRequest,
) -> Result<AddressBookUpdateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    if let Some(ref address) = req.address {
        validate_address(address)?;
    }

    let entry = sdk.address_book_api().update(
        &req.name,
        req.new_name.as_deref(),
        req.address.as_deref(),
        req.memo.as_deref(),
    )?;

    Ok(AddressBookUpdateResponse { entry })
}

pub async fn handle_delete(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookDeleteRequest,
) -> Result<AddressBookDeleteResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    sdk.address_book_api().delete(&req.name)?;

    Ok(AddressBookDeleteResponse {})
}
