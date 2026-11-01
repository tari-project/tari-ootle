// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::{
    models::WebauthnRegistrationPasskeyModel,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};
use tari_utilities::SafePassword;
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;
use webauthn_rs::{
    Webauthn,
    prelude::{
        AuthenticationResult,
        Credential,
        Passkey,
        PasskeyAuthentication,
        PasskeyRegistration,
        RequestChallengeResponse,
        WebauthnError,
    },
};
use zeroize::{Zeroize, Zeroizing};

use crate::services::{SessionData, SessionStore, SessionStoreError};

/// Upper bound on in-progress registration or authentication ceremonies of each kind. Ceremonies are started by
/// unauthenticated callers, and a single-user wallet only ever has a handful in flight. A full store evicts its
/// oldest session, so under sustained ceremony-start traffic an in-flight ceremony survives only this many later
/// starts.
const MAX_PENDING_SESSIONS: usize = 8;

const DEFAULT_PASSKEY_LABEL_PREFIX: &str = "Passkey";

/// File name under `<base_path>` holding the one-time passkey enrollment token while the wallet has no credential.
pub const SETUP_TOKEN_FILE_NAME: &str = "setup_token";

/// Config key incremented whenever passkeys are reset so any in-process JWT secret can be rotated.
pub const PASSKEY_RESET_SEQ_CONFIG_KEY: &str = "webauthn_reset_seq";

/// Number of random bytes encoded in a one-time setup token.
const SETUP_TOKEN_RAW_BYTES: usize = 32;

pub fn setup_token_file_path(base_path: &Path) -> PathBuf {
    base_path.join(SETUP_TOKEN_FILE_NAME)
}

/// Generates a fresh 256-bit URL-safe base64 one-time setup token.
pub fn mint_setup_token() -> SafePassword {
    let mut bytes = Zeroizing::new([0u8; SETUP_TOKEN_RAW_BYTES]);
    rand::rng().fill_bytes(bytes.as_mut_slice());
    let mut encoded = URL_SAFE_NO_PAD.encode(bytes.as_slice());
    let token = SafePassword::from(encoded.as_str());
    encoded.zeroize();
    token
}

/// Compares `presented` against `expected` in constant time without leaking token contents or length.
pub fn verify_setup_token_constant_time(expected: &SafePassword, presented: &str) -> bool {
    let expected_bytes = expected.reveal().as_slice();
    let presented_bytes = presented.as_bytes();

    let expected_digest = Sha256::digest(expected_bytes);
    let presented_digest = Sha256::digest(presented_bytes);

    let digest_eq = expected_digest.as_slice().ct_eq(presented_digest.as_slice());
    let len_eq = expected_bytes.len().ct_eq(&presented_bytes.len());
    bool::from(digest_eq & len_eq)
}

/// RAII guard that writes `<base_path>/setup_token` atomically with mode `0600` on startup and removes it on drop.
#[derive(Debug)]
pub struct SetupTokenFileGuard {
    path: PathBuf,
}

impl SetupTokenFileGuard {
    pub fn write(base_path: &Path, token: &SafePassword) -> anyhow::Result<Self> {
        fs::create_dir_all(base_path)
            .with_context(|| format!("failed to create wallet daemon base directory {}", base_path.display()))?;

        let target_path = setup_token_file_path(base_path);
        let mut temp_file = TempFileBuilder::new()
            .prefix(".setup_token.tmp.")
            .tempfile_in(base_path)
            .with_context(|| format!("failed to create temporary setup_token file in {}", base_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            temp_file
                .as_file()
                .set_permissions(perms)
                .context("failed to set 0600 permissions on temporary setup_token file")?;
        }

        temp_file
            .as_file_mut()
            .write_all(token.reveal().as_slice())
            .context("failed to write setup token to temporary file")?;
        temp_file
            .as_file()
            .sync_all()
            .context("failed to sync temporary setup_token file to disk")?;

        temp_file
            .persist(&target_path)
            .map_err(|err| err.error)
            .with_context(|| format!("failed to persist setup_token file to {}", target_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&target_path, perms)
                .with_context(|| format!("failed to enforce 0600 permissions on {}", target_path.display()))?;
        }

        Ok(Self { path: target_path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove_stale(base_path: &Path) {
        let path = setup_token_file_path(base_path);
        match fs::remove_file(&path) {
            Ok(()) => {},
            Err(err) if err.kind() == ErrorKind::NotFound => {},
            Err(err) => {
                log::warn!(
                    target: "tari::ootle::wallet_daemon::auth",
                    "Failed to remove stale setup_token file {}: {}",
                    path.display(),
                    err
                );
            },
        }
    }
}

impl Drop for SetupTokenFileGuard {
    fn drop(&mut self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {},
            Err(err) if err.kind() == ErrorKind::NotFound => {},
            Err(err) => {
                log::warn!(
                    target: "tari::ootle::wallet_daemon::auth",
                    "Failed to remove setup_token file {} on shutdown: {}",
                    self.path.display(),
                    err
                );
            },
        }
    }
}

/// Removes all stored WebAuthn passkeys and increments the persisted passkey-reset sequence counter so any live
/// daemon instance invalidates outstanding JWTs and refresh tokens. API keys are preserved.
pub fn reset_wallet_passkeys<TStore: WalletStore>(
    wallet_store: &TStore,
    base_path: Option<&Path>,
) -> Result<usize, WalletStorageError> {
    let removed = wallet_store.with_write_tx(|tx| {
        let passkeys = tx.webauthn_reg_list_passkeys()?;
        for entry in &passkeys {
            tx.webauthn_reg_delete_passkey(entry.id)?;
        }
        let prev_seq = tx
            .config_get::<u64>(PASSKEY_RESET_SEQ_CONFIG_KEY)
            .optional()?
            .map(|r| r.value)
            .unwrap_or(0);
        tx.config_set(PASSKEY_RESET_SEQ_CONFIG_KEY, &prev_seq.saturating_add(1), false)?;
        Ok(passkeys.len())
    })?;
    if let Some(dir) = base_path {
        SetupTokenFileGuard::remove_stale(dir);
    }
    Ok(removed)
}

pub fn current_passkey_reset_seq<TStore: WalletStore>(wallet_store: &TStore) -> Result<u64, WalletStorageError> {
    let mut tx = wallet_store.create_read_tx()?;
    Ok(tx
        .config_get::<u64>(PASSKEY_RESET_SEQ_CONFIG_KEY)
        .optional()?
        .map(|r| r.value)
        .unwrap_or(0))
}

#[derive(Debug, Error)]
pub enum WebauthnServiceError {
    #[error("Session store error: {0}")]
    SessionStore(#[from] SessionStoreError),
    #[error("Wallet store error: {0}")]
    WalletStorage(#[from] WalletStorageError),
    #[error("This wallet is already enrolled. A new credential must be added from an authenticated session")]
    AlreadyEnrolled,
    #[error("Invalid or expired setup token")]
    InvalidSetupToken,
    #[error("This credential is already registered in this wallet")]
    DuplicateCredential,
    #[error("Cannot remove the last passkey from the wallet")]
    CannotRemoveLastPasskey,
    #[error("Passkey {id} was not found")]
    PasskeyNotFound { id: u32 },
    #[error("Passkey label must not be empty")]
    EmptyLabel,
    #[error("The credential that authenticated is not enrolled in this wallet")]
    UnknownCredential,
    #[error(
        "The authenticator's signature counter ({presented}) is not above the last one this wallet saw ({stored}). \
         The credential may have been cloned"
    )]
    CounterNotAdvanced { presented: u32, stored: u32 },
    #[error("This wallet has no enrolled credential to log in with. Register one first")]
    NotEnrolled,
    #[error("Webauthn error: {0}")]
    Webauthn(#[from] WebauthnError),
}

#[derive(Clone)]
struct SetupTokenState {
    token: SafePassword,
    created_at: Instant,
    ttl: Duration,
    file_path: Option<PathBuf>,
}

impl SetupTokenState {
    fn new(token: SafePassword, ttl: Duration, file_path: Option<PathBuf>) -> Self {
        Self {
            token,
            created_at: Instant::now(),
            ttl,
            file_path,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }

    fn remove_file(&self) {
        if let Some(ref path) = self.file_path {
            let _ = fs::remove_file(path);
        }
    }
}

impl std::fmt::Debug for SetupTokenState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupTokenState")
            .field("token", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("ttl", &self.ttl)
            .field("file_path", &self.file_path)
            .finish()
    }
}

/// Registration session data
#[derive(Debug, Clone)]
pub(crate) struct RegistrationSessionData {
    username: String,
    label: Option<String>,
    passkey_reg: PasskeyRegistration,
    allow_existing: bool,
    created_at: Instant,
}

impl RegistrationSessionData {
    pub fn new(
        username: String,
        label: Option<String>,
        passkey_reg: PasskeyRegistration,
        allow_existing: bool,
    ) -> Self {
        Self {
            username,
            label,
            passkey_reg,
            allow_existing,
            created_at: Instant::now(),
        }
    }

    pub fn passkey_reg(&self) -> &PasskeyRegistration {
        &self.passkey_reg
    }

    pub fn allow_existing(&self) -> bool {
        self.allow_existing
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
    setup_token: Arc<Mutex<Option<SetupTokenState>>>,
    last_seen_reset_seq: Arc<std::sync::atomic::AtomicU64>,
    session_ttl: Duration,
}

impl<TStore> WebauthnService<TStore>
where TStore: WalletStore
{
    pub fn new(wallet_store: TStore, session_ttl: Duration) -> Self {
        let enrolled = wallet_store
            .create_read_tx()
            .and_then(|mut tx| tx.webauthn_has_any_registration())
            .unwrap_or(false);
        let initial_seq = current_passkey_reset_seq(&wallet_store).unwrap_or(0);
        let initial_token = if enrolled {
            None
        } else {
            Some(SetupTokenState::new(mint_setup_token(), session_ttl, None))
        };
        Self {
            wallet_store,
            registration_sessions: SessionStore::new(session_ttl, MAX_PENDING_SESSIONS),
            auth_sessions: SessionStore::new(session_ttl, MAX_PENDING_SESSIONS),
            setup_token: Arc::new(Mutex::new(initial_token)),
            last_seen_reset_seq: Arc::new(std::sync::atomic::AtomicU64::new(initial_seq)),
            session_ttl,
        }
    }

    fn sync_reset_state(&self) {
        use std::sync::atomic::Ordering;
        let Ok(current_seq) = current_passkey_reset_seq(&self.wallet_store) else {
            return;
        };
        let prev = self.last_seen_reset_seq.load(Ordering::Acquire);
        if current_seq > prev &&
            self.last_seen_reset_seq
                .compare_exchange(prev, current_seq, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            let mut guard = self.setup_token.lock().expect("setup_token mutex poisoned");
            let file_path = guard.take().and_then(|s| s.file_path);
            let fresh_token = mint_setup_token();
            if let Some(ref path) = file_path &&
                let Some(parent) = path.parent()
            {
                let _ = SetupTokenFileGuard::write(parent, &fresh_token);
            }
            *guard = Some(SetupTokenState::new(fresh_token, self.session_ttl, file_path));
        }
    }

    /// True once this wallet holds a credential, whatever username it was enrolled under. The wallet
    /// is single-user, so enrolment is a property of the wallet and the only question the
    /// unauthenticated bootstrap path is allowed to ask.
    pub fn is_enrolled(&self) -> Result<bool, WebauthnServiceError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        Ok(tx.webauthn_has_any_registration()?)
    }

    /// Returns the active one-time setup token when the wallet has no enrolled credential and the token has not
    /// expired or been consumed.
    pub fn setup_token(&self) -> Option<SafePassword> {
        self.sync_reset_state();
        if self.is_enrolled().unwrap_or(true) {
            return None;
        }
        let mut guard = self.setup_token.lock().expect("setup_token mutex poisoned");
        if guard.as_ref().is_some_and(SetupTokenState::is_expired) {
            if let Some(expired) = guard.take() {
                expired.remove_file();
            }
            return None;
        }
        guard.as_ref().map(|state| state.token.clone())
    }

    /// Associates a `<base_path>/setup_token` file path with the active setup token so consuming or expiring the token
    /// removes the file from disk immediately.
    pub fn attach_setup_token_file(&self, path: PathBuf) {
        self.sync_reset_state();
        let mut guard = self.setup_token.lock().expect("setup_token mutex poisoned");
        if let Some(state) = guard.as_mut() {
            state.file_path = Some(path);
        }
    }

    /// Verifies `presented` against the active one-time setup token in constant time without consuming it.
    pub fn verify_setup_token(&self, presented: Option<&str>) -> Result<(), WebauthnServiceError> {
        self.sync_reset_state();
        let presented = presented.ok_or(WebauthnServiceError::InvalidSetupToken)?;
        let mut guard = self.setup_token.lock().expect("setup_token mutex poisoned");
        let Some(state) = guard.as_ref() else {
            return Err(WebauthnServiceError::InvalidSetupToken);
        };
        if state.is_expired() {
            if let Some(expired) = guard.take() {
                expired.remove_file();
            }
            return Err(WebauthnServiceError::InvalidSetupToken);
        }
        if !verify_setup_token_constant_time(&state.token, presented) {
            return Err(WebauthnServiceError::InvalidSetupToken);
        }
        Ok(())
    }

    /// Start registration by creating a new session and saving the temporary [`PasskeyRegistration`],
    /// optional human-readable `label`, and whether existing credentials are allowed (`true` when
    /// initiated by an authenticated `Admin` user session).
    pub async fn start_registration(
        &self,
        username: String,
        label: Option<String>,
        passkey_reg: PasskeyRegistration,
        allow_existing: bool,
    ) -> Result<String, WebauthnServiceError> {
        Ok(self
            .registration_sessions
            .add(RegistrationSessionData::new(
                username,
                label,
                passkey_reg,
                allow_existing,
            ))
            .await?)
    }

    /// Retrieve [`RegistrationSessionData`] by session ID.
    pub async fn get_session(&self, session_id: &str) -> Result<RegistrationSessionData, WebauthnServiceError> {
        let session = self.registration_sessions.get(session_id).await?;
        Ok(session)
    }

    /// Finalizing registration, remove session from store, save passkey (public key of credential) to DB,
    /// and return the newly inserted passkey row ID.
    ///
    /// For an unauthenticated bootstrap ceremony (`allow_existing == false`), this is the enrolment boundary:
    /// the check that the wallet has no credential runs in the same write transaction as the insert. Any check
    /// made before this one is a courtesy rejection — two concurrent finishes both pass it and only this
    /// transaction separates them.
    #[cfg(test)]
    pub async fn finish_registration(&self, session_id: String, passkey: Passkey) -> Result<u32, WebauthnServiceError> {
        let session = self.registration_sessions.remove(session_id.as_str()).await?;
        self.insert_registered_passkey(session, passkey)
    }

    /// Finalizes registration while requiring and consuming the one-time setup token when the session is an
    /// unauthenticated bootstrap registration (`allow_existing == false`).
    pub async fn finish_registration_with_setup_token(
        &self,
        session_id: String,
        passkey: Passkey,
        setup_token: Option<&str>,
    ) -> Result<u32, WebauthnServiceError> {
        self.sync_reset_state();
        let session = self.registration_sessions.remove(session_id.as_str()).await?;
        if session.allow_existing {
            return self.insert_registered_passkey(session, passkey);
        }

        let mut token_guard = self.setup_token.lock().expect("setup_token mutex poisoned");
        let existing_passkeys = self.wallet_passkeys()?;
        if !existing_passkeys.is_empty() {
            return Err(WebauthnServiceError::AlreadyEnrolled);
        }
        let presented = setup_token.ok_or(WebauthnServiceError::InvalidSetupToken)?;
        let Some(state) = token_guard.as_ref() else {
            return Err(WebauthnServiceError::InvalidSetupToken);
        };
        if state.is_expired() {
            if let Some(expired) = token_guard.take() {
                expired.remove_file();
            }
            return Err(WebauthnServiceError::InvalidSetupToken);
        }
        if !verify_setup_token_constant_time(&state.token, presented) {
            return Err(WebauthnServiceError::InvalidSetupToken);
        }

        let id = self.insert_registered_passkey(session, passkey)?;
        if let Some(consumed) = token_guard.take() {
            consumed.remove_file();
        }
        Ok(id)
    }

    fn insert_registered_passkey(
        &self,
        session: RegistrationSessionData,
        passkey: Passkey,
    ) -> Result<u32, WebauthnServiceError> {
        self.wallet_store.with_write_tx(|tx| {
            let existing = tx.webauthn_reg_list_passkeys()?;
            if !session.allow_existing && !existing.is_empty() {
                return Err(WebauthnServiceError::AlreadyEnrolled);
            }
            if existing
                .iter()
                .any(|stored| stored.passkey.cred_id() == passkey.cred_id())
            {
                return Err(WebauthnServiceError::DuplicateCredential);
            }
            let resolved_label = session
                .label
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    let mut next = existing.iter().map(|p| p.id).max().unwrap_or(0) + 1;
                    while existing
                        .iter()
                        .any(|p| p.label == format!("{DEFAULT_PASSKEY_LABEL_PREFIX} {next}"))
                    {
                        next += 1;
                    }
                    format!("{DEFAULT_PASSKEY_LABEL_PREFIX} {next}")
                });
            let id = tx.webauthn_reg_insert(session.username, resolved_label, passkey)?;
            Ok(id)
        })
    }

    /// Every passkey this wallet holds.
    pub fn wallet_passkeys(&self) -> Result<Vec<Passkey>, WebauthnServiceError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        Ok(tx.webauthn_reg_fetch_all_passkeys()?)
    }

    /// Every passkey metadata entry this wallet holds, ordered by creation / id.
    pub fn list_passkeys(&self) -> Result<Vec<WebauthnRegistrationPasskeyModel>, WebauthnServiceError> {
        let mut tx = self.wallet_store.create_read_tx()?;
        Ok(tx.webauthn_reg_list_passkeys()?)
    }

    /// Rename a registered passkey by ID.
    pub fn rename_passkey(&self, id: u32, label: String) -> Result<(), WebauthnServiceError> {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return Err(WebauthnServiceError::EmptyLabel);
        }
        self.wallet_store.with_write_tx(|tx| {
            tx.webauthn_reg_rename_passkey(id, trimmed).map_err(|e| match e {
                WalletStorageError::NotFound { .. } => WebauthnServiceError::PasskeyNotFound { id },
                other => WebauthnServiceError::WalletStorage(other),
            })
        })
    }

    /// Remove a registered passkey by ID. Refuses to remove the last remaining passkey in the wallet.
    pub fn remove_passkey(&self, id: u32) -> Result<(), WebauthnServiceError> {
        self.wallet_store.with_write_tx(|tx| {
            let passkeys = tx.webauthn_reg_list_passkeys()?;
            if !passkeys.iter().any(|entry| entry.id == id) {
                return Err(WebauthnServiceError::PasskeyNotFound { id });
            }
            if passkeys.len() <= 1 {
                return Err(WebauthnServiceError::CannotRemoveLastPasskey);
            }
            tx.webauthn_reg_delete_passkey(id)?;
            Ok(())
        })
    }

    /// Start a login ceremony that any of this wallet's passkeys can complete, returning the challenge
    /// and the session ID that finishes it. The wallet is single-user, so the ceremony is the wallet's
    /// and nothing the caller supplies selects the credentials it offers.
    pub async fn start_authentication(
        &self,
        webauthn: &Webauthn,
    ) -> Result<(RequestChallengeResponse, String), WebauthnServiceError> {
        let passkeys = self.wallet_passkeys()?;
        if passkeys.is_empty() {
            return Err(WebauthnServiceError::NotEnrolled);
        }
        let (challenge, passkey_auth) = webauthn.start_passkey_authentication(passkeys.as_slice())?;
        let session_id = self.auth_sessions.add(AuthSessionData::new(passkey_auth)).await?;
        Ok((challenge, session_id))
    }

    pub async fn auth_passkey(&self, session_id: &str) -> Result<PasskeyAuthentication, WebauthnServiceError> {
        Ok(self.auth_sessions.get(session_id).await?.passkey_auth.clone())
    }

    /// Checks a verified assertion's signature counter against the stored credential and stores the
    /// advanced counter and `last_used_at` timestamp. The login must be refused on error.
    ///
    /// A ceremony checks the counter against the credential as it stood when the ceremony started,
    /// and ceremonies stay open for the session TTL. A login completed in between advances the stored
    /// counter, so this check against the stored value is the one that catches an assertion from a
    /// cloned authenticator. As in the WebAuthn rule, a credential whose counter is zero on both
    /// sides has no counter, as is usual for synced passkeys, and is not checked.
    pub fn record_authentication(&self, result: &AuthenticationResult) -> Result<(), WebauthnServiceError> {
        self.wallet_store.with_write_tx(|tx| {
            let mut passkey = tx
                .webauthn_reg_fetch_all_passkeys()?
                .into_iter()
                .find(|passkey| passkey.cred_id() == result.cred_id())
                .ok_or(WebauthnServiceError::UnknownCredential)?;
            let stored = Credential::from(passkey.clone()).counter;
            let presented = result.counter();
            if (presented > 0 || stored > 0) && presented <= stored {
                return Err(WebauthnServiceError::CounterNotAdvanced { presented, stored });
            }
            let _ = passkey.update_credential(result);
            tx.webauthn_reg_update_passkey(&passkey)?;
            Ok(())
        })
    }

    pub async fn finish_authentication(&self, session_id: &str) -> Result<(), WebauthnServiceError> {
        self.auth_sessions.remove(session_id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
    use url::Url;
    use uuid::Uuid;
    use webauthn_rs::{WebauthnBuilder, prelude::CredentialID};

    use super::*;

    /// A credential in the shape the store round-trips. The enrolment gate turns on a registration
    /// being present, so no signature over this key is ever checked.
    fn passkey() -> Passkey {
        serde_json::from_value(serde_json::json!({
            "cred": {
                "cred_id": "AAECAwQFBgcICQoLDA0ODw",
                "cred": {
                    "type_": "ES256",
                    "key": {
                        "EC_EC2": {
                            "curve": "SECP256R1",
                            "x": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
                            "y": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
                        }
                    }
                },
                "counter": 0,
                "transports": null,
                "user_verified": true,
                "backup_eligible": false,
                "backup_state": false,
                "registration_policy": "required",
                "extensions": {},
                "attestation": { "data": "None", "metadata": "None" },
                "attestation_format": "none"
            }
        }))
        .unwrap()
    }

    fn webauthn() -> Webauthn {
        WebauthnBuilder::new("localhost", &Url::parse("http://localhost:5100").unwrap())
            .unwrap()
            .build()
            .unwrap()
    }

    /// The in-progress registration state a real `reg_start` produces. The service only carries it
    /// from session to session, it never inspects it.
    fn passkey_reg(username: &str) -> PasskeyRegistration {
        let (_, passkey_reg) = webauthn()
            .start_passkey_registration(Uuid::new_v4(), username, username, None)
            .unwrap();
        passkey_reg
    }

    fn service() -> (WebauthnService<SqliteWalletStore>, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let store = SqliteWalletStore::try_open(temp.path().join("wallet.sqlite")).unwrap();
        store.run_migrations().unwrap();
        (WebauthnService::new(store, Duration::from_secs(60)), temp)
    }

    async fn enrol(service: &WebauthnService<SqliteWalletStore>, username: &str) -> Result<u32, WebauthnServiceError> {
        let session_id = service
            .start_registration(username.to_string(), None, passkey_reg(username), false)
            .await
            .unwrap();
        service.finish_registration(session_id, passkey()).await
    }

    #[tokio::test]
    async fn first_enrolment_succeeds() {
        let (service, _temp) = service();
        assert!(!service.is_enrolled().unwrap());
        enrol(&service, "owner").await.unwrap();
        assert!(service.is_enrolled().unwrap());
    }

    fn allowed_credential_ids(challenge: &RequestChallengeResponse) -> Vec<serde_json::Value> {
        challenge
            .public_key
            .allow_credentials
            .iter()
            .map(|allowed| serde_json::to_value(&allowed.id).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn login_challenge_offers_the_wallet_credential() {
        let (service, _temp) = service();
        enrol(&service, "owner").await.unwrap();

        let (challenge, _) = service.start_authentication(&webauthn()).await.unwrap();
        assert_eq!(allowed_credential_ids(&challenge), vec![
            serde_json::to_value(passkey().cred_id()).unwrap()
        ]);
    }

    #[tokio::test]
    async fn login_is_refused_before_enrolment() {
        let (service, _temp) = service();
        let err = service.start_authentication(&webauthn()).await.unwrap_err();
        assert!(matches!(err, WebauthnServiceError::NotEnrolled));
    }

    #[tokio::test]
    async fn enrolment_under_another_username_is_refused() {
        let (service, _temp) = service();
        enrol(&service, "owner").await.unwrap();

        let err = enrol(&service, "attacker").await.unwrap_err();
        assert!(matches!(err, WebauthnServiceError::AlreadyEnrolled));
        assert_eq!(service.wallet_passkeys().unwrap().len(), 1);
    }

    /// What `finish_passkey_authentication` reports for a ceremony by `cred_id` at `counter`.
    fn authentication_result(cred_id: &CredentialID, counter: u32) -> AuthenticationResult {
        serde_json::from_value(serde_json::json!({
            "cred_id": cred_id,
            "needs_update": true,
            "user_verified": true,
            "backup_state": false,
            "backup_eligible": false,
            "counter": counter,
            "extensions": {},
        }))
        .unwrap()
    }

    /// A ceremony by the enrolled credential at `counter`.
    fn enrolled_at(counter: u32) -> AuthenticationResult {
        authentication_result(passkey().cred_id(), counter)
    }

    fn stored_counter(service: &WebauthnService<SqliteWalletStore>) -> u32 {
        let passkeys = service.wallet_passkeys().unwrap();
        assert_eq!(passkeys.len(), 1);
        Credential::from(passkeys[0].clone()).counter
    }

    #[tokio::test]
    async fn an_authentication_advances_the_stored_counter() {
        let (service, _temp) = service();
        enrol(&service, "owner").await.unwrap();
        assert_eq!(stored_counter(&service), 0);

        service.record_authentication(&enrolled_at(5)).unwrap();
        assert_eq!(stored_counter(&service), 5);
        service.record_authentication(&enrolled_at(6)).unwrap();
        assert_eq!(stored_counter(&service), 6);
    }

    /// A ceremony started before a later login completed checked its counter against the credential
    /// as it stood at its start. Completing it with a counter at or below the one that login stored
    /// is what a cloned authenticator does, and must be refused.
    #[tokio::test]
    async fn a_counter_at_or_below_the_stored_one_is_refused() {
        let (service, _temp) = service();
        enrol(&service, "owner").await.unwrap();
        service.record_authentication(&enrolled_at(10)).unwrap();

        for counter in [3, 10] {
            let err = service.record_authentication(&enrolled_at(counter)).unwrap_err();
            assert!(
                matches!(err, WebauthnServiceError::CounterNotAdvanced { presented, stored: 10 } if presented == counter),
                "{counter}: {err}"
            );
        }
        assert_eq!(stored_counter(&service), 10);
    }

    #[tokio::test]
    async fn a_credential_without_a_counter_is_not_checked() {
        let (service, _temp) = service();
        enrol(&service, "owner").await.unwrap();

        service.record_authentication(&enrolled_at(0)).unwrap();
        service.record_authentication(&enrolled_at(0)).unwrap();
        assert_eq!(stored_counter(&service), 0);
    }

    #[tokio::test]
    async fn an_authentication_by_an_unknown_credential_is_refused() {
        let (service, _temp) = service();
        enrol(&service, "owner").await.unwrap();

        let unknown = CredentialID::from(vec![0xff; 16]);
        let err = service
            .record_authentication(&authentication_result(&unknown, 5))
            .unwrap_err();
        assert!(matches!(err, WebauthnServiceError::UnknownCredential));
        assert_eq!(stored_counter(&service), 0);
    }

    /// Two enrolments that both passed any earlier check still end with one credential, because the
    /// count and the insert share a write transaction. Atomicity here comes from [`SqliteWalletStore`]
    /// holding its single connection's mutex for the transaction's lifetime: were the store ever to
    /// move to a connection pool, the deferred `BEGIN` would let two readers both count zero.
    #[tokio::test]
    async fn second_enrolment_is_refused_within_the_write_tx() {
        let (service, _temp) = service();

        let first = service
            .start_registration("owner".to_string(), None, passkey_reg("owner"), false)
            .await
            .unwrap();
        let second = service
            .start_registration("attacker".to_string(), None, passkey_reg("attacker"), false)
            .await
            .unwrap();

        let (a, b) = tokio::join!(
            service.finish_registration(first, passkey()),
            service.finish_registration(second, passkey())
        );

        assert_eq!(
            [a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(),
            1,
            "exactly one of the two enrolments must be accepted"
        );
        assert_eq!(service.wallet_passkeys().unwrap().len(), 1);
    }

    fn passkey_with_cred_id(cred_id_b64: &str) -> Passkey {
        serde_json::from_value(serde_json::json!({
            "cred": {
                "cred_id": cred_id_b64,
                "cred": {
                    "type_": "ES256",
                    "key": {
                        "EC_EC2": {
                            "curve": "SECP256R1",
                            "x": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
                            "y": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
                        }
                    }
                },
                "counter": 0,
                "transports": null,
                "user_verified": true,
                "backup_eligible": false,
                "backup_state": false,
                "registration_policy": "required",
                "extensions": {},
                "attestation": { "data": "None", "metadata": "None" },
                "attestation_format": "none"
            }
        }))
        .unwrap()
    }

    async fn enrol_additional(
        service: &WebauthnService<SqliteWalletStore>,
        username: &str,
        label: Option<&str>,
        pk: Passkey,
    ) -> Result<u32, WebauthnServiceError> {
        let session_id = service
            .start_registration(
                username.to_string(),
                label.map(str::to_string),
                passkey_reg(username),
                true,
            )
            .await
            .unwrap();
        service.finish_registration(session_id, pk).await
    }

    #[tokio::test]
    async fn authenticated_session_can_register_multiple_passkeys_and_login_offers_all() {
        let (service, _temp) = service();

        // First bootstrap registration generates "Passkey 1" when no label is given
        let first_session = service
            .start_registration("owner".to_string(), None, passkey_reg("owner"), false)
            .await
            .unwrap();
        let first_pk = passkey_with_cred_id("AAECAwQFBgcICQoLDA0ODw");
        let first_id = service
            .finish_registration(first_session, first_pk.clone())
            .await
            .unwrap();

        // Second registration under the same username with explicit label
        let second_pk = passkey_with_cred_id("EBESExQVFhcYGRobHB0eHw");
        let second_id = enrol_additional(&service, "owner", Some("YubiKey Backup"), second_pk.clone())
            .await
            .unwrap();
        assert_ne!(first_id, second_id);

        // Re-registering the same credential ID is rejected
        let dup_err = enrol_additional(&service, "owner", Some("Duplicate"), second_pk.clone())
            .await
            .unwrap_err();
        assert!(matches!(dup_err, WebauthnServiceError::DuplicateCredential));

        // Listing returns both passkeys in order with their labels
        let listed = service.list_passkeys().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, first_id);
        assert_eq!(listed[0].label, "Passkey 1");
        assert!(listed[0].last_used_at.is_none());
        assert_eq!(listed[1].id, second_id);
        assert_eq!(listed[1].label, "YubiKey Backup");
        assert!(listed[1].last_used_at.is_none());

        // Login challenge offers both credentials
        let (challenge, _) = service.start_authentication(&webauthn()).await.unwrap();
        assert_eq!(allowed_credential_ids(&challenge), vec![
            serde_json::to_value(first_pk.cred_id()).unwrap(),
            serde_json::to_value(second_pk.cred_id()).unwrap(),
        ]);

        // Counters and last_used_at are tracked independently per passkey
        service
            .record_authentication(&authentication_result(second_pk.cred_id(), 7))
            .unwrap();
        let listed = service.list_passkeys().unwrap();
        assert_eq!(Credential::from(listed[0].passkey.clone()).counter, 0);
        assert!(listed[0].last_used_at.is_none());
        assert_eq!(Credential::from(listed[1].passkey.clone()).counter, 7);
        assert!(listed[1].last_used_at.is_some());

        // Zero-counter synced passkey still records last_used_at on login
        service
            .record_authentication(&authentication_result(first_pk.cred_id(), 0))
            .unwrap();
        let listed = service.list_passkeys().unwrap();
        assert!(listed[0].last_used_at.is_some());
    }

    #[tokio::test]
    async fn rename_and_remove_passkeys_enforce_invariants_and_default_labels_do_not_repeat() {
        let (service, _temp) = service();
        let first_id = enrol(&service, "owner").await.unwrap();

        let second_pk = passkey_with_cred_id("EBESExQVFhcYGRobHB0eHw");
        let second_id = enrol_additional(&service, "owner", None, second_pk).await.unwrap();
        let listed = service.list_passkeys().unwrap();
        assert_eq!(listed[0].label, "Passkey 1");
        assert_eq!(listed[1].label, "Passkey 2");

        // Remove first passkey ("Passkey 1") while "Passkey 2" (id=2) remains, then add a third without a label.
        // The new default label must not repeat "Passkey 2".
        service.remove_passkey(first_id).unwrap();
        let third_pk = passkey_with_cred_id("ICQoLDA0ODxAREhMUFRYXA");
        let third_id = enrol_additional(&service, "owner", None, third_pk).await.unwrap();
        let listed = service.list_passkeys().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, second_id);
        assert_eq!(listed[0].label, "Passkey 2");
        assert_eq!(listed[1].id, third_id);
        assert_eq!(listed[1].label, "Passkey 3");

        // Rename second passkey
        service
            .rename_passkey(second_id, "  Hardware Security Key  ".to_string())
            .unwrap();
        let listed = service.list_passkeys().unwrap();
        assert_eq!(listed[0].label, "Hardware Security Key");

        // Empty label is rejected
        let empty_err = service.rename_passkey(second_id, "   ".to_string()).unwrap_err();
        assert!(matches!(empty_err, WebauthnServiceError::EmptyLabel));

        // Unknown ID is rejected on rename and remove
        let missing_rename = service.rename_passkey(9999, "Valid".to_string()).unwrap_err();
        assert!(matches!(missing_rename, WebauthnServiceError::PasskeyNotFound {
            id: 9999
        }));
        let missing_remove = service.remove_passkey(9999).unwrap_err();
        assert!(matches!(missing_remove, WebauthnServiceError::PasskeyNotFound {
            id: 9999
        }));

        // Removing one of two passkeys succeeds
        service.remove_passkey(third_id).unwrap();
        let remaining = service.list_passkeys().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, second_id);

        // Removing the last remaining passkey is refused
        let last_err = service.remove_passkey(second_id).unwrap_err();
        assert!(matches!(last_err, WebauthnServiceError::CannotRemoveLastPasskey));
        assert_eq!(service.list_passkeys().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn setup_token_file_is_written_and_removed_on_consumption_or_drop() {
        let (service, temp) = service();
        let token = service.setup_token().expect("unenrolled wallet has a setup token");
        let token_str = std::str::from_utf8(token.reveal()).unwrap().to_string();

        let guard = SetupTokenFileGuard::write(temp.path(), &token).unwrap();
        let file_path = guard.path().to_path_buf();
        assert_eq!(file_path, setup_token_file_path(temp.path()));
        assert_eq!(fs::read_to_string(&file_path).unwrap(), token_str);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "setup_token file must be mode 0600");
        }

        service.attach_setup_token_file(file_path.clone());
        let session_id = service
            .start_registration("owner".to_string(), None, passkey_reg("owner"), false)
            .await
            .unwrap();
        service
            .finish_registration_with_setup_token(session_id, passkey(), Some(token_str.as_str()))
            .await
            .unwrap();

        assert!(
            !file_path.exists(),
            "setup_token file must be deleted when registration consumes the token"
        );
        assert!(service.setup_token().is_none());
        drop(guard);
    }

    #[tokio::test]
    async fn setup_token_expires_after_ttl() {
        let temp = tempfile::tempdir().unwrap();
        let store = SqliteWalletStore::try_open(temp.path().join("wallet.sqlite")).unwrap();
        store.run_migrations().unwrap();
        let service = WebauthnService::new(store, Duration::from_millis(5));
        let token = service.setup_token().unwrap();
        let token_str = std::str::from_utf8(token.reveal()).unwrap().to_string();

        tokio::time::sleep(Duration::from_millis(20)).await;
        let err = service.verify_setup_token(Some(token_str.as_str())).unwrap_err();
        assert!(matches!(err, WebauthnServiceError::InvalidSetupToken));
        assert!(service.setup_token().is_none());
    }
}
