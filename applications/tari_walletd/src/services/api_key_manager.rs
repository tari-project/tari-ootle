//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use rand::{RngCore, thread_rng};
use sha2::{Digest, Sha256};
use tari_ootle_wallet_sdk::storage::{CommittableStore, ReadableWalletStore, WalletStorageError, WriteableWalletStore};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_ootle_wallet_storage_sqlite::models::ApiKey;
use tari_ootle_walletd_client::permissions::{JrpcPermission, JrpcPermissions};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ApiKeyManager {
    wallet_store: SqliteWalletStore,
}

impl ApiKeyManager {
    pub fn new(wallet_store: SqliteWalletStore) -> Self {
        Self { wallet_store }
    }

    pub async fn create(
        &self,
        name: String,
        requested_permissions: Vec<JrpcPermission>,
        caller_permissions: &JrpcPermissions,
        grant_admin_explicit: bool,
    ) -> Result<(ApiKeyInfo, String), ApiKeyError> {
        if !caller_permissions.has_permission(&JrpcPermission::Admin) {
            return Err(ApiKeyError::PermissionDenied);
        }

        for permission in &requested_permissions {
            if !caller_permissions.has_permission(permission) {
                return Err(ApiKeyError::ScopeExceedsGrantor);
            }
        }

        if requested_permissions.contains(&JrpcPermission::Admin) && !grant_admin_explicit {
            return Err(ApiKeyError::AdminScopeRequiresConfirmation);
        }

        let (plaintext, hash) = generate_api_key();
        let permissions = serde_json::to_string(&requested_permissions).map_err(|e| {
            ApiKeyError::Storage(WalletStorageError::EncodingError {
                operation: "api_key_create",
                item: "requested_permissions",
                details: e.to_string(),
            })
        })?;

        let created_at = current_unix_timestamp()?;
        let api_key = ApiKey {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            key_hash: hash,
            scopes: permissions,
            created_at,
            expires_at: None,
            last_used: None,
            revoked: 0,
        };

        let mut tx = self.wallet_store.create_write_tx()?;
        tx.insert_api_key(&api_key)?;
        tx.commit()?;

        Ok((
            ApiKeyInfo {
                id: api_key.id.clone(),
                name: api_key.name.clone(),
                permissions: requested_permissions,
                created_at: u64::try_from(api_key.created_at).map_err(|e| {
                    ApiKeyError::Storage(WalletStorageError::DecodingError {
                        operation: "api_key_create",
                        item: "created_at",
                        details: e.to_string(),
                    })
                })?,
                expires_at: None,
                last_used: None,
                revoked: false,
            },
            plaintext,
        ))
    }

    pub async fn authenticate(&self, raw_key: &str) -> Result<ApiKeyInfo, ApiKeyError> {
        let hash = hash_api_key(raw_key);
        let mut tx = self.wallet_store.create_write_tx()?;
        let key = tx.get_api_key_by_hash(&hash)?.ok_or(ApiKeyError::InvalidKey)?;
        if key.revoked == 1 {
            return Err(ApiKeyError::Revoked);
        }
        tx.touch_api_key_last_used(&key.id)?;
        tx.commit()?;

        let permissions = serde_json::from_str(&key.scopes).map_err(|e| {
            ApiKeyError::Storage(WalletStorageError::DecodingError {
                operation: "api_key_authenticate",
                item: "scopes",
                details: e.to_string(),
            })
        })?;

        Ok(ApiKeyInfo {
            id: key.id,
            name: key.name,
            permissions,
            created_at: u64::try_from(key.created_at).map_err(|e| {
                ApiKeyError::Storage(WalletStorageError::DecodingError {
                    operation: "api_key_authenticate",
                    item: "created_at",
                    details: e.to_string(),
                })
            })?,
            expires_at: key
                .expires_at
                .map(|value| {
                    u64::try_from(value).map_err(|e| {
                        ApiKeyError::Storage(WalletStorageError::DecodingError {
                            operation: "api_key_authenticate",
                            item: "expires_at",
                            details: e.to_string(),
                        })
                    })
                })
                .transpose()?,
            last_used: key
                .last_used
                .map(|value| {
                    u64::try_from(value).map_err(|e| {
                        ApiKeyError::Storage(WalletStorageError::DecodingError {
                            operation: "api_key_authenticate",
                            item: "last_used",
                            details: e.to_string(),
                        })
                    })
                })
                .transpose()?,
            revoked: false,
        })
    }

    pub async fn list(&self) -> Result<Vec<ApiKeyInfo>, ApiKeyError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        tx.list_api_keys()?
            .into_iter()
            .map(|key| {
                let permissions = serde_json::from_str(&key.scopes).map_err(|e| {
                    ApiKeyError::Storage(WalletStorageError::DecodingError {
                        operation: "api_key_list",
                        item: "scopes",
                        details: e.to_string(),
                    })
                })?;

                Ok(ApiKeyInfo {
                    id: key.id,
                    name: key.name,
                    permissions,
                    created_at: u64::try_from(key.created_at).map_err(|e| {
                        ApiKeyError::Storage(WalletStorageError::DecodingError {
                            operation: "api_key_list",
                            item: "created_at",
                    details: e.to_string(),
                })
            })?,
                    expires_at: key
                        .expires_at
                        .map(|value| {
                            u64::try_from(value).map_err(|e| {
                                ApiKeyError::Storage(WalletStorageError::DecodingError {
                                    operation: "api_key_list",
                                    item: "expires_at",
                                    details: e.to_string(),
                                })
                            })
                        })
                        .transpose()?,
                    last_used: key
                        .last_used
                        .map(|value| {
                            u64::try_from(value).map_err(|e| {
                                ApiKeyError::Storage(WalletStorageError::DecodingError {
                                    operation: "api_key_list",
                                    item: "last_used",
                                    details: e.to_string(),
                                })
                            })
                        })
                        .transpose()?,
                    revoked: key.revoked != 0,
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn revoke(&self, id: &str) -> Result<(), ApiKeyError> {
        let mut tx = self.wallet_store.create_write_tx()?;
        if !tx.revoke_api_key(id)? {
            return Err(ApiKeyError::NotFound);
        }
        tx.commit()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub permissions: Vec<JrpcPermission>,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub last_used: Option<u64>,
    pub revoked: bool,
}

#[derive(Debug, Error)]
pub enum ApiKeyError {
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Requested scopes exceed the grantor permissions")]
    ScopeExceedsGrantor,
    #[error("Admin scope requires explicit confirmation")]
    AdminScopeRequiresConfirmation,
    #[error("Invalid API key")]
    InvalidKey,
    #[error("API key revoked")]
    Revoked,
    #[error("API key not found")]
    NotFound,
    #[error("Storage error: {0}")]
    Storage(#[from] WalletStorageError),
}

fn generate_api_key() -> (String, String) {
    let mut bytes = [0u8; 32];
    thread_rng().fill_bytes(&mut bytes);
    let key = format!("tak_{}", BASE64_URL_SAFE_NO_PAD.encode(bytes));
    let hash = hash_api_key(&key);
    (key, hash)
}

fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

fn current_unix_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
