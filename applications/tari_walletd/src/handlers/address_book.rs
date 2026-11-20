//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use axum_extra::headers::authorization::Bearer;
use tari_ootle_address::{Network, OotleAddress};
use tari_ootle_walletd_client::{
    permissions::{Crud, Permission},
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

use crate::handlers::{
    HandlerContext,
    helpers::{OrJrpcNotFound, invalid_params},
};

fn validate_address(address: &str, network: Network) -> Result<(), anyhow::Error> {
    let parsed = OotleAddress::from_str(address)
        .map_err(|e| invalid_params("address", Some(format!("Invalid Ootle address '{address}': {e}"))))?;
    if parsed.network() != network {
        return Err(invalid_params(
            "address",
            Some(format!(
                "Address network mismatch: address is for {:?} but wallet is configured for {:?}",
                parsed.network(),
                network,
            )),
        ));
    }
    parsed
        .validate()
        .map_err(|e| invalid_params("address", Some(format!("Invalid Ootle address '{address}': {e}"))))?;
    Ok(())
}

pub async fn handle_add(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookAddRequest,
) -> Result<AddressBookAddResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::AddressBook(Crud::Create)])?;

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
    context.authorize(token, &[Permission::AddressBook(Crud::Read)])?;

    let entries = sdk.address_book_api().list()?;

    Ok(AddressBookListResponse { entries })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookGetRequest,
) -> Result<AddressBookGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::AddressBook(Crud::Read)])?;

    let entry = sdk.address_book_api().get(&req.name).or_jrpc_not_found()?;

    Ok(AddressBookGetResponse { entry })
}

pub async fn handle_update(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookUpdateRequest,
) -> Result<AddressBookUpdateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::AddressBook(Crud::Update)])?;

    if let Some(ref address) = req.address {
        validate_address(address, sdk.network())?;
    }

    let entry = sdk
        .address_book_api()
        .update(
            &req.name,
            req.new_name.as_deref(),
            req.address.as_deref(),
            req.note.as_deref(),
        )
        .or_jrpc_not_found()?;

    Ok(AddressBookUpdateResponse { entry })
}

pub async fn handle_delete(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AddressBookDeleteRequest,
) -> Result<AddressBookDeleteResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::AddressBook(Crud::Delete)])?;

    sdk.address_book_api().delete(&req.name).or_jrpc_not_found()?;

    Ok(AddressBookDeleteResponse {})
}

#[cfg(test)]
mod tests {
    use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
    use ootle_byte_type::ToByteType;
    use tari_ootle_wallet_sdk::models::KeyBranch;
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::*;
    use crate::{handlers::test_support::TestDaemon, server::ApplicationErrorCode};

    #[tokio::test]
    async fn address_book_rejects_non_canonical_keys_and_maps_missing_entries_to_not_found() {
        let daemon = TestDaemon::start().await;
        let sdk = daemon.context.wallet_sdk();
        let view_key = sdk.key_manager_api().next_key(KeyBranch::ViewOnlyKey).unwrap();
        let non_canonical_address = OotleAddress::new(
            Network::LocalNet,
            view_key.to_public_key().to_byte_type(),
            RistrettoPublicKeyBytes::from([2u8; 32]),
        )
        .to_bech32_string();

        let err = handle_add(
            &daemon.context,
            Some(&daemon.session),
            AddressBookAddRequest {
                name: "bob".to_string(),
                address: non_canonical_address,
                note: None,
            },
        )
        .await
        .unwrap_err();
        let jrpc = err.downcast_ref::<JsonRpcError>().expect("JsonRpcError");
        assert!(matches!(jrpc.error_reason(), JsonRpcErrorReason::InvalidParams));

        for err in [
            handle_get(
                &daemon.context,
                Some(&daemon.session),
                AddressBookGetRequest {
                    name: "missing".to_string(),
                },
            )
            .await
            .unwrap_err(),
            handle_update(
                &daemon.context,
                Some(&daemon.session),
                AddressBookUpdateRequest {
                    name: "missing".to_string(),
                    new_name: None,
                    address: None,
                    note: Some("note".to_string()),
                },
            )
            .await
            .unwrap_err(),
            handle_delete(
                &daemon.context,
                Some(&daemon.session),
                AddressBookDeleteRequest {
                    name: "missing".to_string(),
                },
            )
            .await
            .unwrap_err(),
        ] {
            let jrpc = err.downcast_ref::<JsonRpcError>().expect("JsonRpcError");
            assert!(matches!(
                jrpc.error_reason(),
                JsonRpcErrorReason::ApplicationError(code) if code == ApplicationErrorCode::NotFound as i32
            ));
        }
    }
}
