//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum_extra::headers::authorization::Bearer;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, TokenData, Validation};
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use tari_ootle_walletd_client::{
    permissions::{Claims, JrpcPermission, JrpcPermissions},
    types::EncodedJwtString,
};

pub struct JwtApi<'a> {
    default_expiry: Duration,
    jwt_secret_key: &'a SafePassword,
}

impl<'a> JwtApi<'a> {
    pub(crate) fn new(default_expiry: Duration, jwt_secret_key: &'a SafePassword) -> Self {
        Self {
            default_expiry,
            jwt_secret_key,
        }
    }

    pub fn generate_auth_claims(&self, permissions: JrpcPermissions) -> Result<Claims, JwtApiError> {
        self.generate_auth_claims_for_api_key(permissions, None)
    }

    pub fn generate_auth_claims_for_api_key(
        &self,
        permissions: JrpcPermissions,
        api_key_id: Option<i32>,
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

    pub fn check_auth(
        &self,
        token: Option<&Bearer>,
        req_permissions: &[JrpcPermission],
    ) -> Result<Claims, JwtApiError> {
        let token = token.ok_or(JwtApiError::AccessDeniedNoBearerToken)?;
        let token_data = self.decode_jwt(token.token())?;
        let token_permissions = &token_data.claims.permissions;
        if token_permissions.has_permission(&JrpcPermission::Admin) {
            return Ok(token_data.claims);
        }
        for permission in req_permissions {
            if !token_permissions.has_permission(permission) {
                return Err(JwtApiError::InsufficientPermissions {
                    required: permission.clone(),
                });
            }
        }
        Ok(token_data.claims)
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
    #[error("Access denied. API key has been revoked")]
    AccessDeniedApiKeyRevoked,
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
