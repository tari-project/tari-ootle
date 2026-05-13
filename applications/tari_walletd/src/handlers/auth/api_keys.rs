//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::Context;
use axum_extra::headers::authorization::Bearer;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};
use tari_ootle_wallet_sdk::{
    models::ApiKey,
    storage::{
        ReadableWalletStore, WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter,
        WriteableWalletStore,
    },
};
use tari_ootle_walletd_client::{
    permissions::{JrpcPermission, JrpcPermissions},
    types::{
        AuthApiKeyInfo, AuthCreateApiKeyRequest, AuthCreateApiKeyResponse, AuthListApiKeysRequest,
        AuthListApiKeysResponse, AuthRevokeApiKeyRequest, AuthRevokeApiKeyResponse, EncodedJwtString,
    },
};

use crate::handlers::{
    HandlerContext,
    helpers::{invalid_request, unauthorized},
};

const API_KEY_PREFIX: &str = "twda_";
const API_KEY_BYTES: usize = 32;
const API_KEY_HASH_DOMAIN: &[u8] = b"tari.walletd.api_key.v1\0";
const API_KEY_NAME_MAX_LEN: usize = 64;

#[derive(Debug, Clone)]
pub struct AuthenticatedApiKey {
    pub id: i32,
    pub permissions: JrpcPermissions,
}

pub fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(API_KEY_HASH_DOMAIN);
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn is_api_key_secret(raw_key: &str) -> bool {
    raw_key.starts_with(API_KEY_PREFIX)
}

fn mint_api_key() -> EncodedJwtString {
    let mut bytes = [0u8; API_KEY_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    EncodedJwtString::new(format!("{}{}", API_KEY_PREFIX, URL_SAFE_NO_PAD.encode(bytes)))
}

pub fn authenticate_api_key<TStore>(store: &TStore, raw_key: &str) -> Result<AuthenticatedApiKey, anyhow::Error>
where
    TStore: WalletStore,
{
    let key_hash = hash_api_key(raw_key);
    let api_key = store
        .with_read_tx(|tx| map_invalid_key(tx.api_keys_find_active_by_hash(&key_hash)))
        .map_err(|_| unauthorized("Invalid or revoked API key"))?;

    // Touch after lookup using an active-only update. If a revoke races this
    // login, the touch fails and we refuse to mint even a short-lived JWT.
    let api_key = store
        .with_write_tx(|tx| map_invalid_key(tx.api_keys_touch_last_used(api_key.id)))
        .map_err(|_| unauthorized("Invalid or revoked API key"))?;

    Ok(AuthenticatedApiKey {
        id: api_key.id,
        permissions: decode_permissions(&api_key.permissions)
            .with_context(|| format!("Persisted API key {} has invalid permissions", api_key.id))?,
    })
}

pub async fn handle_create_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthCreateApiKeyRequest,
) -> Result<AuthCreateApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    validate_create_request(&request)?;

    let raw_key = mint_api_key();
    let key_hash = hash_api_key(raw_key.as_str());
    let permissions_json = encode_permissions(&request.permissions)?;
    let key = context.wallet_sdk().store().with_write_tx(|tx| {
        tx.api_keys_create(request.name.trim(), &key_hash, &permissions_json)
            .map_err(anyhow::Error::from)
    })?;

    Ok(AuthCreateApiKeyResponse {
        api_key: raw_key,
        key: api_key_info(key)?,
    })
}

pub async fn handle_list_api_keys(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _request: AuthListApiKeysRequest,
) -> Result<AuthListApiKeysResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let keys = context
        .wallet_sdk()
        .store()
        .with_read_tx(|tx| tx.api_keys_list().map_err(anyhow::Error::from))?
        .into_iter()
        .map(api_key_info)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuthListApiKeysResponse { api_keys: keys })
}

pub async fn handle_revoke_api_key(
    context: &HandlerContext,
    token: Option<&Bearer>,
    request: AuthRevokeApiKeyRequest,
) -> Result<AuthRevokeApiKeyResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    context
        .wallet_sdk()
        .store()
        .with_write_tx(|tx| tx.api_keys_revoke(request.id).map(|_| ()).map_err(anyhow::Error::from))?;
    Ok(AuthRevokeApiKeyResponse {})
}

fn validate_create_request(request: &AuthCreateApiKeyRequest) -> Result<(), anyhow::Error> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err(invalid_request("API key name cannot be empty"));
    }
    if name.len() > API_KEY_NAME_MAX_LEN {
        return Err(invalid_request("API key name is too long (max 64 characters)"));
    }
    if request.permissions.is_empty() {
        return Err(invalid_request("API key must include at least one permission"));
    }
    if request.permissions.contains(&JrpcPermission::Admin) && !request.confirm_admin {
        return Err(invalid_request(
            "Granting Admin to an API key requires confirm_admin = true",
        ));
    }
    Ok(())
}

fn encode_permissions(permissions: &[JrpcPermission]) -> Result<String, anyhow::Error> {
    serde_json::to_string(permissions).context("Failed to encode API key permissions")
}

fn decode_permissions(encoded: &str) -> Result<JrpcPermissions, anyhow::Error> {
    let permissions: Vec<JrpcPermission> =
        serde_json::from_str(encoded).context("Failed to decode API key permissions")?;
    Ok(permissions.into())
}

fn api_key_info(api_key: ApiKey) -> Result<AuthApiKeyInfo, anyhow::Error> {
    Ok(AuthApiKeyInfo {
        id: api_key.id,
        name: api_key.name,
        permissions: decode_permissions(&api_key.permissions)?.into_vec(),
        created_at: api_key.created_at,
        last_used_at: api_key.last_used_at,
        expires_at: api_key.expires_at,
        revoked_at: api_key.revoked_at,
    })
}

fn map_invalid_key(result: Result<ApiKey, WalletStorageError>) -> Result<ApiKey, WalletStorageError> {
    result.map_err(|err| match err {
        WalletStorageError::NotFound { .. } => WalletStorageError::NotFound {
            operation: "api_key_authenticate",
            entity: "api_key".to_string(),
            key: "<redacted>".to_string(),
        },
        err => err,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_keys_are_prefixed_url_safe_and_unique() {
        let first = mint_api_key();
        let second = mint_api_key();
        assert!(is_api_key_secret(first.as_str()));
        assert!(first.starts_with(API_KEY_PREFIX));
        assert!(second.starts_with(API_KEY_PREFIX));
        assert_ne!(first.as_str(), second.as_str());
        assert_eq!(first.len(), API_KEY_PREFIX.len() + 43);
    }

    #[test]
    fn hash_is_domain_separated_and_stable() {
        assert_eq!(hash_api_key("twda_secret"), hash_api_key("twda_secret"));
        assert_ne!(
            hash_api_key("twda_secret"),
            Sha256::digest(b"twda_secret")
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
    }

    #[test]
    fn permissions_round_trip_through_json() {
        let permissions = vec![
            JrpcPermission::AccountInfo,
            JrpcPermission::TransactionGet,
            JrpcPermission::AddressBook(tari_ootle_walletd_client::permissions::AddressBookPermission::Read),
        ];
        let encoded = encode_permissions(&permissions).unwrap();
        let decoded = decode_permissions(&encoded).unwrap();
        for permission in permissions {
            assert!(decoded.has_permission(&permission));
        }
    }

    #[test]
    fn create_request_rejects_overlong_name() {
        let request = AuthCreateApiKeyRequest {
            name: "a".repeat(API_KEY_NAME_MAX_LEN + 1),
            permissions: vec![JrpcPermission::AccountInfo],
            confirm_admin: false,
        };

        assert!(validate_create_request(&request).is_err());
    }
}
