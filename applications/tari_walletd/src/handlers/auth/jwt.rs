//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum_extra::headers::authorization::Bearer;
use jsonwebtoken::{errors, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_wallet_sdk::storage::{
    CommittableStore,
    WalletStorageError,
    WalletStore,
    WalletStoreReader,
    WalletStoreWriter,
};
use tari_wallet_daemon_client::{
    permissions::{Claims, JrpcPermission, JrpcPermissions},
    types::EncodedJwtString,
};

pub struct JwtApi<'a, TStore> {
    store: &'a TStore,
    default_expiry: Duration,
    auth_secret_key: SafePassword,
    jwt_secret_key: SafePassword,
}

// This is used when you request permission.
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthClaims {
    id: u64,
    permissions: JrpcPermissions,
    exp: u64,
}

impl<'a, TStore: WalletStore> JwtApi<'a, TStore> {
    pub(crate) fn new(
        store: &'a TStore,
        default_expiry: Duration,
        auth_secret_key: SafePassword,
        jwt_secret_key: SafePassword,
    ) -> Self {
        Self {
            store,
            default_expiry,
            auth_secret_key,
            jwt_secret_key,
        }
    }

    // Get and also increment index. We could probably use random id here.
    pub fn get_index(&self) -> Result<u64, JwtApiError> {
        let mut tx = self.store.create_write_tx()?;
        let index = tx.jwt_add_empty_token()?;
        tx.commit()?;
        Ok(index)
    }

    pub fn generate_auth_token(
        &self,
        permissions: JrpcPermissions,
        duration: Option<Duration>,
    ) -> Result<(EncodedJwtString, Duration), JwtApiError> {
        let id = self.get_index()?;
        let valid_till = SystemTime::now() + duration.unwrap_or(self.default_expiry);
        let exp = valid_till
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtApiError::InvalidExpiry)?;
        let my_claims = AuthClaims {
            id,
            permissions,
            exp: exp.as_secs(),
        };
        let auth_token = jsonwebtoken::encode(
            &Header::default(),
            &my_claims,
            // TODO: jsonwebtoken leaks the secret in memory after drop (https://github.com/Keats/jsonwebtoken/issues/337)
            // This **should** be not that bad since the secret we use is changed on each startup. However, if the user
            // can set their own secret key, this is bad.
            &EncodingKey::from_secret(self.auth_secret_key.reveal()),
        )?;
        Ok((auth_token.into(), exp))
    }

    fn check_auth_token(&self, auth_token: &str) -> Result<AuthClaims, JwtApiError> {
        let auth_token_data = jsonwebtoken::decode::<AuthClaims>(
            auth_token,
            &DecodingKey::from_secret(self.auth_secret_key.reveal()),
            &Validation::default(),
        )?;
        Ok(auth_token_data.claims)
    }

    fn get_token_claims(&self, token: &str) -> Result<Claims, JwtApiError> {
        let claims = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret_key.reveal()),
            &Validation::default(),
        )
        .map(|token_data| token_data.claims)?;
        Ok(claims)
    }

    fn get_permissions(&self, token: &Bearer) -> Result<JrpcPermissions, JwtApiError> {
        self.get_token_claims(token.token()).map(|claims| claims.permissions)
    }

    pub fn grant(&self, name: String, auth_token: &str) -> Result<EncodedJwtString, JwtApiError> {
        let auth_claims = self.check_auth_token(auth_token)?;
        let my_claims = Claims {
            id: auth_claims.id,
            name,
            permissions: auth_claims.permissions,
            exp: auth_claims.exp,
        };
        let permissions_token = jsonwebtoken::encode(
            &Header::default(),
            &my_claims,
            &EncodingKey::from_secret(self.jwt_secret_key.reveal()),
        )?;
        let mut tx = self.store.create_write_tx()?;

        tx.jwt_store_decision(auth_claims.id, Some(&permissions_token))?;
        tx.commit()?;
        Ok(permissions_token.into())
    }

    pub fn deny(&self, auth_token: &str) -> Result<(), JwtApiError> {
        let auth_claims = self.check_auth_token(auth_token)?;
        let mut tx = self.store.create_write_tx()?;
        tx.jwt_store_decision(auth_claims.id, None)?;
        tx.commit()?;
        Ok(())
    }

    fn is_token_revoked(&self, token: &Bearer) -> Result<bool, JwtApiError> {
        let mut tx = self.store.create_write_tx()?;
        let revoked = tx.jwt_is_revoked(token.token())?;
        tx.commit()?;
        Ok(revoked)
    }

    pub fn check_auth(&self, token: Option<&Bearer>, req_permissions: &[JrpcPermission]) -> Result<(), JwtApiError> {
        let token = token.ok_or(JwtApiError::AccessDeniedNoBearerToken)?;
        if self.is_token_revoked(token)? {
            return Err(JwtApiError::TokenRevoked {});
        }
        let token_permissions = self.get_permissions(token)?;
        for permission in req_permissions {
            if !token_permissions.has_permission(permission) &&
                !token_permissions.has_permission(&JrpcPermission::Admin)
            {
                return Err(JwtApiError::InsufficientPermissions {
                    required: permission.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn revoke(&self, token_id: i32) -> Result<(), JwtApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.jwt_revoke(token_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_tokens(&self) -> Result<Vec<Claims>, JwtApiError> {
        let mut tx = self.store.create_read_tx()?;
        let tokens = tx.jwt_get_all()?;
        let mut res = Vec::new();
        for (_, token) in tokens.iter().filter(|(_, token)| token.is_some()) {
            if let Ok(claims) = self.get_token_claims(token.as_ref().unwrap().as_str()) {
                res.push(claims);
            }
        }
        Ok(res)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JwtApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("JWT error : {0}")]
    JwtError(#[from] errors::Error),
    #[error("Access denied. No bearer token provided")]
    AccessDeniedNoBearerToken,
    #[error("Insufficient permissions. Required '{required:?}'")]
    InsufficientPermissions { required: JrpcPermission },
    #[error("Token revoked")]
    TokenRevoked,
    #[error("Invalid expiry")]
    InvalidExpiry,
}
