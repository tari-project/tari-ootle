// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, Instant};

use tari_ootle_wallet_sdk::storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter};
use thiserror::Error;
use webauthn_rs::prelude::{Passkey, PasskeyAuthentication, PasskeyRegistration};

use crate::services::{SessionData, SessionStore, SessionStoreError};

#[derive(Debug, Error)]
pub enum WebauthnServiceError {
    #[error("Session store error: {0}")]
    SessionStore(#[from] SessionStoreError),
    #[error("Wallet store error: {0}")]
    WalletStorage(#[from] WalletStorageError),
}

/// Registration session data
#[derive(Debug, Clone)]
pub(crate) struct RegistrationSessionData {
    username: String,
    passkey_reg: PasskeyRegistration,
    created_at: Instant,
}

impl RegistrationSessionData {
    pub fn new(username: String, passkey_reg: PasskeyRegistration) -> Self {
        Self {
            username,
            passkey_reg,
            created_at: Instant::now(),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn passkey_reg(&self) -> &PasskeyRegistration {
        &self.passkey_reg
    }
}

impl SessionData for RegistrationSessionData {
    fn created_at(&self) -> Instant {
        self.created_at
    }
}

/// Authentication session data
#[derive(Debug, Clone)]
struct AuthSessionData {
    passkey_auth: PasskeyAuthentication,
    created_at: Instant,
}

impl AuthSessionData {
    pub fn new(passkey_auth: PasskeyAuthentication) -> Self {
        Self {
            passkey_auth,
            created_at: Instant::now(),
        }
    }
}

impl SessionData for AuthSessionData {
    fn created_at(&self) -> Instant {
        self.created_at
    }
}

/// A service to store temporary registration data (between `start` and `finish` RPC calls)
/// and save the result in DB when finished.
#[derive(Debug, Clone)]
pub struct WebauthnService<TStore> {
    wallet_store: TStore,
    registration_sessions: SessionStore<RegistrationSessionData>,
    auth_sessions: SessionStore<AuthSessionData>,
}

impl<TStore> WebauthnService<TStore>
where TStore: WalletStore
{
    pub fn new(wallet_store: TStore, session_ttl: Duration) -> Self {
        Self {
            wallet_store,
            registration_sessions: SessionStore::new(session_ttl),
            auth_sessions: SessionStore::new(session_ttl),
        }
    }

    pub fn is_user_registered(&self, username: &str) -> Result<bool, WebauthnServiceError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        Ok(tx.webauthn_is_user_registered(username)?)
    }

    /// Start registration by creating a new session and save the temporary [`PasskeyRegistration`].
    pub async fn start_registration(
        &self,
        username: String,
        passkey_reg: PasskeyRegistration,
    ) -> Result<String, WebauthnServiceError> {
        Ok(self
            .registration_sessions
            .add(RegistrationSessionData::new(username, passkey_reg))
            .await?)
    }

    /// Retrieve [`PasskeyRegistration`] by session ID.
    pub async fn get_session(&self, session_id: &str) -> Result<RegistrationSessionData, WebauthnServiceError> {
        let session = self.registration_sessions.get(session_id).await?;
        Ok(session)
    }

    /// Finalizing registration, remove session from store and save passkey (public key of credential) to DB
    pub async fn finish_registration(&self, session_id: String, passkey: Passkey) -> Result<(), WebauthnServiceError> {
        let session = self.registration_sessions.remove(session_id.as_str()).await?;
        let mut tx = self.wallet_store.create_write_tx()?;
        tx.webauthn_reg_insert(session.username, passkey)?;
        tx.commit()?;
        Ok(())
    }

    /// Fetch passkeys for a username.
    pub fn passkeys(&self, username: String) -> Result<Vec<Passkey>, WebauthnServiceError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        Ok(tx.webauthn_reg_fetch_passkeys(username)?)
    }

    pub async fn start_authentication(
        &self,
        passkey_auth: PasskeyAuthentication,
    ) -> Result<String, WebauthnServiceError> {
        Ok(self.auth_sessions.add(AuthSessionData::new(passkey_auth)).await?)
    }

    pub async fn auth_passkey(&self, session_id: &str) -> Result<PasskeyAuthentication, WebauthnServiceError> {
        Ok(self.auth_sessions.get(session_id).await?.passkey_auth.clone())
    }

    pub async fn finish_authentication(&self, session_id: &str) -> Result<(), WebauthnServiceError> {
        self.auth_sessions.remove(session_id).await?;
        Ok(())
    }
}
