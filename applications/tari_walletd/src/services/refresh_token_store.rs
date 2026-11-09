//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::Arc,
    time::{Duration, Instant},
};

use axum_extra::extract::cookie::{Cookie, SameSite};
use tari_ootle_walletd_client::{
    permissions::{Claims, Permissions},
    types::RefreshTokenHash,
};
use tokio::sync::RwLock;
use zeroize::{Zeroize, ZeroizeOnDrop};

const MAX_REFRESH_TOKENS: usize = 1024;

#[derive(Clone)]
pub struct RefreshTokenStore {
    tokens: Arc<RwLock<HashMap<RefreshTokenHash, RefreshTokenData>>>,
    expiry: Duration,
    max_tokens: usize,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct RefreshToken(Box<uuid::Bytes>);

impl RefreshToken {
    pub fn into_cookie(self, name: &str) -> Cookie<'_> {
        let mut cookie = Cookie::new(name, uuid::Uuid::from_bytes_ref(&self.0).to_string());
        cookie.set_http_only(true);
        cookie.set_same_site(SameSite::Strict);
        cookie
    }
}

impl AsRef<[u8]> for RefreshToken {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(Debug)]
struct RefreshTokenData {
    claims: Claims,
    expires_at: Instant,
}

impl RefreshTokenStore {
    pub fn new(expiry: Duration) -> Self {
        Self {
            tokens: Default::default(),
            expiry,
            max_tokens: MAX_REFRESH_TOKENS,
        }
    }

    #[cfg(test)]
    pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub async fn new_token(&self, permissions: Permissions, exp: u64) -> RefreshToken {
        let mut write = self.tokens.write().await;
        clear_expired_tokens(&mut write);
        let max_tokens = self.max_tokens.max(1);
        while write.len() >= max_tokens {
            let Some(oldest_key) = write.iter().min_by_key(|(_, data)| data.expires_at).map(|(k, _)| *k) else {
                break;
            };
            write.remove(&oldest_key);
        }
        let data = RefreshTokenData {
            claims: Claims {
                permissions,
                exp,
                delegated: false,
                delegation_id: None,
            },
            expires_at: Instant::now() + self.expiry,
        };

        let token = RefreshToken(Box::new(uuid::Uuid::new_v4().into_bytes()));
        write.insert(hash_token(&token), data);
        token
    }

    pub async fn validate_token_str(&self, token: &str) -> Option<Claims> {
        let token = uuid::Uuid::parse_str(token)
            .ok()
            .map(|b| RefreshToken(Box::new(b.into_bytes())))?;
        let hashed_token = hash_token(&token);
        let read = self.tokens.read().await;
        if let Some(data) = read.get(&hashed_token) &&
            Instant::now() < data.expires_at
        {
            return Some(data.claims.clone());
        }
        None
    }

    pub async fn revoke_token(&self, token: &RefreshTokenHash) -> bool {
        let mut write = self.tokens.write().await;
        clear_expired_tokens(&mut write);
        write.remove(token).is_some()
    }

    pub async fn clear_expired(&self) {
        let mut write = self.tokens.write().await;
        clear_expired_tokens(&mut write);
    }

    pub async fn to_vec(&self) -> Vec<(RefreshTokenHash, Claims)> {
        let now = Instant::now();
        let read = self.tokens.read().await;
        read.iter()
            .filter(|(_, v)| v.expires_at > now)
            .map(|(k, v)| (*k, v.claims.clone()))
            .collect()
    }
}

fn hash_token(token: &RefreshToken) -> RefreshTokenHash {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_ref());
    let result = hasher.finalize();
    RefreshTokenHash::new(result.into())
}

fn clear_expired_tokens(tokens: &mut HashMap<RefreshTokenHash, RefreshTokenData>) {
    let now = Instant::now();
    tokens.retain(|_, data| data.expires_at > now);
}

impl Debug for RefreshTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshTokenStore")
            .field("tokens", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_token_store_caps_capacity_and_filters_expired_entries() {
        let store = RefreshTokenStore::new(Duration::from_secs(60)).with_max_tokens(2);
        let first = store.new_token(Permissions::none(), 1).await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        let second = store.new_token(Permissions::none(), 2).await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        let third = store.new_token(Permissions::none(), 3).await;

        let first_str = uuid::Uuid::from_bytes_ref(&first.0).to_string();
        let second_str = uuid::Uuid::from_bytes_ref(&second.0).to_string();
        let third_str = uuid::Uuid::from_bytes_ref(&third.0).to_string();

        assert!(store.validate_token_str(&first_str).await.is_none());
        assert!(store.validate_token_str(&second_str).await.is_some());
        assert!(store.validate_token_str(&third_str).await.is_some());
        assert_eq!(store.to_vec().await.len(), 2);

        let short_store = RefreshTokenStore::new(Duration::from_millis(1));
        short_store.new_token(Permissions::none(), 1).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(short_store.to_vec().await.is_empty());
    }
}
