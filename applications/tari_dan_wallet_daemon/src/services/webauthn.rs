// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use crate::services::{SessionData, SessionStore, SessionStoreError};
use std::time::{Duration, Instant};
use tari_dan_wallet_sdk::storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter};
use thiserror::Error;
use webauthn_rs::prelude::{Passkey, PasskeyRegistration};

#[derive(Debug, Error)]
pub enum WebauthnServiceError {
    #[error("Session store error: {0}")]
    SessionStore(#[from] SessionStoreError),
    #[error("Wallet store error: {0}")]
    WalletStorage(#[from] WalletStorageError),
}

/// Registration session data
#[derive(Debug, Clone)]
struct RegistrationSessionData {
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
}

impl SessionData for RegistrationSessionData {
    fn created_at(&self) -> Instant {
        self.created_at
    }
}

/// Authentication session data
#[derive(Debug, Clone)]
struct AuthSessionData {
    passkey: PasskeyRegistration,
    created_at: Instant,
}

impl AuthSessionData {
    pub fn new(passkey: PasskeyRegistration) -> Self {
        Self {
            passkey,
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
pub struct WebauthnService<TStore>
where TStore: WalletStore,
{
    wallet_store: TStore,
    registration_sessions: SessionStore<RegistrationSessionData>,
    auth_sessions: SessionStore<AuthSessionData>,
}

impl<TStore> WebauthnService<TStore>
where TStore: WalletStore, {
    pub fn new(wallet_store: TStore, session_ttl: Duration) -> Self {
        Self {
            wallet_store,
            registration_sessions: SessionStore::new(session_ttl),
            auth_sessions: SessionStore::new(session_ttl),
        }
    }

    pub async fn registration_count(&self) -> Result<u64, WebauthnServiceError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        Ok(tx.webauthn_reg_count()?)
    }

    /// Start registration by creating a new session and save the temporary [`PasskeyRegistration`].
    pub async fn start_registration(&self, username: String, passkey_reg: PasskeyRegistration)
        -> Result<String, WebauthnServiceError> {
        Ok(
            self.registration_sessions.add(RegistrationSessionData::new(username, passkey_reg)).await?
        )
    }

    /// Retrieve [`PasskeyRegistration`] by session ID.
    pub async fn registration_passkey(&self, session_id: String) -> Result<PasskeyRegistration, WebauthnServiceError> {
        let session = self.registration_sessions.get(session_id.as_str()).await?;
        Ok(session.passkey_reg.clone())
    }

    /// Finalizing registration, remove session from store and save passkey (public key of credential) to DB
    pub async fn finish_registration(&self, session_id: String, passkey: Passkey) -> Result<(), WebauthnServiceError> {
        let username = self.registration_sessions.remove(session_id.as_str()).await
            .ok_or(WebauthnServiceError::SessionStore(SessionStoreError::SessionNotFound {session_id}))?
            .username;
        let mut tx = self.wallet_store.create_write_tx()?;
        tx.webauthn_reg_insert(username, passkey)?;
        Ok(())
    }
}