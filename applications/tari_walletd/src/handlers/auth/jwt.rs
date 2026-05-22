//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum_extra::headers::authorization::Bearer;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, TokenData, Validation};
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use tari_ootle_walletd_client::{
    permissions::{Claims, Permission, Permissions},
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

    pub fn generate_auth_claims(&self, permissions: Permissions) -> Result<Claims, AuthError> {
        let valid_till = SystemTime::now() + self.default_expiry;
        let exp = valid_till
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::InvalidExpiry)?;
        Ok(Claims {
            permissions,
            exp: exp.as_secs(),
        })
    }

    fn decode_jwt(&self, token: &str) -> Result<TokenData<Claims>, AuthError> {
        let token_data = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret_key.reveal()),
            &Validation::default(),
        )?;
        Ok(token_data)
    }

    pub fn grant(&self, claims: &Claims) -> Result<EncodedJwtString, AuthError> {
        let permissions_token = jsonwebtoken::encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.jwt_secret_key.reveal()),
        )?;

        Ok(permissions_token.into())
    }

    pub fn check_auth(&self, token: Option<&Bearer>, req_permissions: &[Permission]) -> Result<(), AuthError> {
        let token = token.ok_or(AuthError::AccessDeniedNoBearerToken)?;
        let token_data = self.decode_jwt(token.token())?;
        enforce_scopes(&token_data.claims.permissions, req_permissions)
    }
}

/// Apply the resource-aware matcher to a granted set. Shared by the JWT path
/// and the bearer-API-key path so they enforce the exact same policy.
/// `Permissions::satisfies` folds in Admin, mutation-implies-read, and scope
/// coverage — see `permissions.rs` for the full semantics.
pub fn enforce_scopes(granted: &Permissions, required: &[Permission]) -> Result<(), AuthError> {
    for permission in required {
        if !granted.satisfies(permission) {
            return Err(AuthError::InsufficientPermissions {
                required: permission.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
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
    // Same wording for "no such key" and "key revoked" so an attacker can't
    // distinguish via the error string. Mapped to the same 401 Unauthorized
    // status as the other bearer-token failures.
    #[error("Access denied. API key is invalid or revoked")]
    ApiKeyInvalidOrRevoked,
    // The endpoint requires an interactive user session (WebAuthn) — API
    // keys are deliberately excluded so a leaked Admin key cannot mint or
    // revoke further keys, limiting blast radius of a compromise to the
    // lifetime of that single key.
    #[error("Access denied. This endpoint requires an interactive user session, not an API key")]
    UserAuthOnly,
    #[error("Insufficient permissions. Required '{required:?}'")]
    InsufficientPermissions { required: Permission },
    #[error("Invalid expiry")]
    InvalidExpiry,
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        match err.kind() {
            jsonwebtoken::errors::ErrorKind::InvalidToken => Self::AccessDeniedInvalidBearerToken,
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => Self::AccessDeniedExpiredToken,
            _ => Self::JwtError(err),
        }
    }
}
