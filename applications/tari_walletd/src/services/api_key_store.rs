//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Persistent API key store for agent-friendly wallet authentication.
//!
//! API keys are stored as SHA-256 hashes — the raw key is only shown once at creation.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tari_ootle_walletd_client::permissions::{JrpcPermission, JrpcPermissions};
use tari_ootle_walletd_client::types::{ApiKeyId, ApiKeyInfo};
use time::PrimitiveDateTime;
use tokio::sync::RwLock;

const LOG_TARGET: &str = "tari::ootle::walletd::api_key_store";

/// A generated API key along with its metadata. Returned only at creation time.
#[derive(Debug, Clone)]
pub struct GeneratedApiKey {
    /// The raw API key string (shown exactly once).
    pub raw_key: String,
    /// Metadata about the key.
    pub info: ApiKeyInfo,
}

/// Internal representation of a stored API key.
#[derive(Clone, Serialize, Deserialize)]
struct StoredApiKey {
    id: [u8; 32],
    name: String,
    scopes: Vec<String>,
    created_at: u64,
    last_used_at: Option<u64>,
}

impl StoredApiKey {
    fn to_info(&self) -> ApiKeyInfo {
        let scopes = self.scopes.iter().filter_map(|s| JrpcPermission::from_str(s).ok()).collect();
        ApiKeyInfo {
            id: ApiKeyId::new(self.id),
            name: self.name.clone(),
            scopes,
            created_at: PrimitiveDateTime::UNIX_EPOCH + time::Duration::seconds(self.created_at as i64),
            last_used_at: self.last_used_at.map(|ts| {
                PrimitiveDateTime::UNIX_EPOCH + time::Duration::seconds(ts as i64)
            }),
        }
    }
}

/// Thread-safe API key store.
#[derive(Clone)]
pub struct ApiKeyStore {
    keys: Arc<RwLock<HashMap<[u8; 32], StoredApiKey>>>,
}

impl ApiKeyStore {
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate a new API key with the given name and scopes.
    /// Returns the raw key (shown once) and metadata.
    pub async fn create_key(
        &self,
        name: String,
        scopes: Vec<JrpcPermission>,
    ) -> GeneratedApiKey {
        let mut id = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut id);

        let raw_key = generate_raw_key();
        let hashed = hash_key(&raw_key);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let scope_strings: Vec<String> = scopes.iter().map(|p| p.to_string()).collect();

        let stored = StoredApiKey {
            id,
            name: name.clone(),
            scopes: scope_strings,
            created_at: now,
            last_used_at: None,
        };

        self.keys.write().await.insert(hashed, stored);

        let info = ApiKeyInfo {
            id: ApiKeyId::new(id),
            name,
            scopes,
            created_at: PrimitiveDateTime::UNIX_EPOCH + time::Duration::seconds(now as i64),
            last_used_at: None,
        };

        GeneratedApiKey { raw_key, info }
    }

    /// Validate an API key and return its permissions. Updates last_used timestamp.
    pub async fn validate_key(&self, raw_key: &str) -> Option<JrpcPermissions> {
        let hashed = hash_key(raw_key);
        let mut write = self.keys.write().await;

        if let Some(entry) = write.get_mut(&hashed) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            entry.last_used_at = Some(now);

            let scopes: JrpcPermissions = entry
                .scopes
                .iter()
                .filter_map(|s| JrpcPermission::from_str(s).ok())
                .collect();

            Some(scopes)
        } else {
            None
        }
    }

    /// List all active API keys.
    pub async fn list_keys(&self) -> Vec<ApiKeyInfo> {
        let read = self.keys.read().await;
        read.values().map(|k| k.to_info()).collect()
    }

    /// Revoke an API key by its ID.
    pub async fn revoke_key(&self, id: ApiKeyId) -> bool {
        let mut write = self.keys.write().await;
        let target = *id.as_bytes();
        write.retain(|_, v| v.id != target);
        true
    }

    /// Verify the caller has admin permission (key includes Admin scope).
    pub async fn is_admin_key(&self, raw_key: &str) -> bool {
        let hashed = hash_key(raw_key);
        let read = self.keys.read().await;
        read.get(&hashed).map_or(false, |k| {
            k.scopes.iter().any(|s| s == "Admin")
        })
    }
}

/// Generate a cryptographically secure raw API key string.
fn generate_raw_key() -> String {
    let mut bytes = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("tari_api_{}", hex::encode(bytes))
}

/// Hash an API key for storage.
fn hash_key(raw_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hasher.finalize().into()
}

impl std::fmt::Debug for ApiKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyStore")
            .field("keys", &"<redacted>")
            .finish()
    }
}
