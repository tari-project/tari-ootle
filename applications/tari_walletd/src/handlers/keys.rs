//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::hex::Hex,
};
use tari_ootle_wallet_sdk::apis::key_manager::KeyManagerApiError;
use tari_ootle_walletd_client::{
    permissions::{Crud, Permission},
    types::{KeysImportRequest, KeysImportResponse},
};

use super::{
    context::HandlerContext,
    helpers::{invalid_params, invalid_request},
};

pub async fn handle_import(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: KeysImportRequest,
) -> Result<KeysImportResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::Keys(Crud::Create)])?;
    if req.secret_key == RistrettoSecretKey::default() {
        return Err(invalid_params("secret_key", Some("Secret key cannot be zero")));
    }
    let public_key = RistrettoPublicKey::from_secret_key(&req.secret_key);
    let key_id = sdk
        .key_manager_api()
        .import_key(&public_key.to_hex(), &req.secret_key, req.key_type)
        .map_err(|e| match e {
            KeyManagerApiError::KeyAlreadyImportedWithDifferentType { .. } => invalid_request(e),
            e => e.into(),
        })?;
    Ok(KeysImportResponse {
        key_id,
        public_key: public_key.to_byte_type(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::TestDaemon;

    #[tokio::test]
    async fn import_rejects_zero_secret_key() {
        use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
        use tari_ootle_wallet_sdk::models::KeyType;

        let daemon = TestDaemon::start().await;
        let session = Some(&daemon.session);

        let err = handle_import(&daemon.context, session, KeysImportRequest {
            secret_key: RistrettoSecretKey::default(),
            key_type: KeyType::Owner,
        })
        .await
        .unwrap_err();
        let jrpc_err = err
            .downcast_ref::<JsonRpcError>()
            .unwrap_or_else(|| panic!("expected JsonRpcError, got: {err}"));
        assert!(matches!(jrpc_err.error_reason(), JsonRpcErrorReason::InvalidParams));
        assert!(err.to_string().contains("secret_key"), "{err}");
    }
}
