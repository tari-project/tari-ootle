//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum_extra::headers::authorization::Bearer;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, TokenData, Validation};
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_wallet_sdk::storage::{ReadableWalletStore, WalletStorageError};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_ootle_walletd_client::{
    permissions::{Claims, JrpcPermission, JrpcPermissions},
    types::EncodedJwtString,
};

pub struct JwtApi<'a> {
    default_expiry: Duration,
    jwt_secret_key: &'a SafePassword,
    wallet_store: SqliteWalletStore,
}

impl<'a> JwtApi<'a> {
    pub(crate) fn new(
        default_expiry: Duration,
        jwt_secret_key: &'a SafePassword,
        wallet_store: SqliteWalletStore,
    ) -> Self {
        Self {
            default_expiry,
            jwt_secret_key,
            wallet_store,
        }
    }

    pub fn generate_auth_claims(&self, permissions: JrpcPermissions) -> Result<Claims, JwtApiError> {
        self.generate_bound_auth_claims(permissions, None)
    }

    pub fn generate_auth_claims_for_api_key(
        &self,
        permissions: JrpcPermissions,
        api_key_id: String,
    ) -> Result<Claims, JwtApiError> {
        self.generate_bound_auth_claims(permissions, Some(api_key_id))
    }

    fn generate_bound_auth_claims(
        &self,
        permissions: JrpcPermissions,
        api_key_id: Option<String>,
    ) -> Result<Claims, JwtApiError> {
        let valid_till = SystemTime::now() + self.default_expiry;
        let exp = valid_till
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtApiError::InvalidExpiry)?;
        Ok(Claims {
            permissions,
            exp: exp.as_secs(),
            api_key_id,
        })
    }

    fn decode_jwt(&self, token: &str) -> Result<TokenData<Claims>, JwtApiError> {
        let token_data = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret_key.reveal()),
            &Validation::default(),
        )?;
        Ok(token_data)
    }

    pub fn grant(&self, claims: &Claims) -> Result<EncodedJwtString, JwtApiError> {
        let permissions_token = jsonwebtoken::encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.jwt_secret_key.reveal()),
        )?;

        Ok(permissions_token.into())
    }

    pub fn check_auth(&self, token: Option<&Bearer>, req_permissions: &[JrpcPermission]) -> Result<(), JwtApiError> {
        let token = token.ok_or(JwtApiError::AccessDeniedNoBearerToken)?;
        let claims = self.claims_from_bearer(token)?;
        self.ensure_api_key_active(&claims)?;
        let token_permissions = &claims.permissions;
        if token_permissions.has_permission(&JrpcPermission::Admin) {
            return Ok(());
        }
        for permission in req_permissions {
            if !token_permissions.has_permission(permission) {
                return Err(JwtApiError::InsufficientPermissions {
                    required: permission.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn claims_from_bearer(&self, token: &Bearer) -> Result<Claims, JwtApiError> {
        Ok(self.decode_jwt(token.token())?.claims)
    }

    fn ensure_api_key_active(&self, claims: &Claims) -> Result<(), JwtApiError> {
        let Some(api_key_id) = claims.api_key_id.as_deref() else {
            return Ok(());
        };

        let mut tx = self.wallet_store.create_read_tx()?;
        let api_key = tx.get_active_api_key_by_id(api_key_id)?;
        if api_key.is_none() {
            return Err(JwtApiError::AccessDeniedRevokedApiKey);
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JwtApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("JWT error: {0}")]
    JwtError(jsonwebtoken::errors::Error),
    #[error("Access denied. No bearer token provided")]
    AccessDeniedNoBearerToken,
    #[error("Access denied. Invalid bearer token")]
    AccessDeniedInvalidBearerToken,
    #[error("Access denied. Expired token")]
    AccessDeniedExpiredToken,
    #[error("Access denied. API key revoked")]
    AccessDeniedRevokedApiKey,
    #[error("Insufficient permissions. Required '{required:?}'")]
    InsufficientPermissions { required: JrpcPermission },
    #[error("Invalid expiry")]
    InvalidExpiry,
}

impl From<jsonwebtoken::errors::Error> for JwtApiError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        match err.kind() {
            jsonwebtoken::errors::ErrorKind::InvalidToken => Self::AccessDeniedInvalidBearerToken,
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => Self::AccessDeniedExpiredToken,
            _ => Self::JwtError(err),
        }
    }
}
