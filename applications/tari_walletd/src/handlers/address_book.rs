//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use tari_ootle_address::OotleAddress;
use tari_ootle_common_types::Network;
use tari_ootle_walletd_client::{
    permissions::{AddressBookPermission, JrpcPermission},
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

fn validate_address(address: &str, network: Network) -> Result<(), anyhow::Error> {
    let parsed = OotleAddress::from_str(address).map_err(|e| anyhow!("Invalid Ootle address '{address}': {e}"))?;
    if parsed.network() != network {
        return Err(anyhow!(
            "Address network mismatch: address is for {:?} but wallet is configured for {:?}",
            parsed.network(),
            network,
        ));
    }
    Ok(())
}

pub async fn handle_add(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookAddRequest,
) -> Result<AddressBookAddResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::AddressBook(AddressBookPermission::Create)])?;

    validate_address(&req.address, sdk.network())?;

    let entry = sdk
        .address_book_api()
        .add(&req.name, &req.address, req.note.as_deref())?;

    Ok(AddressBookAddResponse { entry })
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _req: AddressBookListRequest,
) -> Result<AddressBookListResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::AddressBook(AddressBookPermission::Read)])?;

    let entries = sdk.address_book_api().list()?;

    Ok(AddressBookListResponse { entries })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookGetRequest,
) -> Result<AddressBookGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::AddressBook(AddressBookPermission::Read)])?;

    let entry = sdk.address_book_api().get(&req.name)?;

    Ok(AddressBookGetResponse { entry })
}

pub async fn handle_update(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookUpdateRequest,
) -> Result<AddressBookUpdateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::AddressBook(AddressBookPermission::Update)])?;

    if let Some(ref address) = req.address {
        validate_address(address, sdk.network())?;
    }

    let entry = sdk.address_book_api().update(
        &req.name,
        req.new_name.as_deref(),
        req.address.as_deref(),
        req.note.as_deref(),
    )?;

    Ok(AddressBookUpdateResponse { entry })
}

pub async fn handle_delete(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookDeleteRequest,
) -> Result<AddressBookDeleteResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::AddressBook(AddressBookPermission::Delete)])?;

    sdk.address_book_api().delete(&req.name)?;

    Ok(AddressBookDeleteResponse {})
}
