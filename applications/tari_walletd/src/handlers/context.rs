//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum_extra::headers::authorization::Bearer;
use dashmap::DashMap;
use tari_ootle_transaction::{Transaction, TransactionBuilder};
use tari_ootle_wallet_sdk::models::WalletEvent;
use tari_ootle_wallet_sdk_services::{
    account_monitor::AccountMonitorHandle,
    notify::Notify,
    transaction_service::TransactionServiceHandle,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_ootle_walletd_client::permissions::JrpcPermission;
use tari_shutdown::ShutdownSignal;
use tari_utilities::SafePassword;
use webauthn_rs::Webauthn;

use crate::{
    WalletSdk,
    config::WalletDaemonConfig,
    handlers::auth::{
        WalletAuthenticator,
        api_keys,
        jwt::{AuthError, JwtApi, enforce_scopes},
    },
    services::{RefreshTokenStore, WebauthnService},
};

#[derive(Debug, Clone)]
pub struct HandlerContext {
    wallet_sdk: WalletSdk,
    notifier: Notify<WalletEvent>,
    transaction_service: TransactionServiceHandle,
    account_monitor: AccountMonitorHandle,
    config: WalletDaemonConfig,
    jwt_secret: SafePassword,
    authenticator: WalletAuthenticator,
    refresh_token_store: RefreshTokenStore,
    shutdown_signal: ShutdownSignal,
    /// In-memory map of `api_key.id` → most-recent `last_used_at` bump time.
    /// The auth shim runs on every authenticated request, so the hot path
    /// must avoid spawning a tokio task and grabbing the SQLite write lock
    /// when a recent bump has already happened. This table lets the common
    /// case short-circuit with a single sharded read; the DB-level
    /// throttle in `api_key_touch_last_used` stays as belt-and-braces
    /// against process restart and racing shim invocations.
    api_key_last_used_bumps: Arc<DashMap<i32, Instant>>,
}

impl HandlerContext {
    pub fn new(
        wallet_sdk: WalletSdk,
        notifier: Notify<WalletEvent>,
        transaction_service: TransactionServiceHandle,
        account_monitor: AccountMonitorHandle,
        config: WalletDaemonConfig,
        authenticator: WalletAuthenticator,
        jwt_secret: SafePassword,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            wallet_sdk,
            notifier,
            transaction_service,
            account_monitor,
            config,
            authenticator,
            refresh_token_store: RefreshTokenStore::new(Duration::from_secs(60 * 60)),
            jwt_secret,
            shutdown_signal,
            api_key_last_used_bumps: Arc::new(DashMap::new()),
        }
    }

    pub fn notifier(&self) -> &Notify<WalletEvent> {
        &self.notifier
    }

    pub fn wallet_sdk(&self) -> &WalletSdk {
        &self.wallet_sdk
    }

    /// Authorise an incoming request. Bearer tokens prefixed with the API
    /// key marker (`tw_…`) are resolved directly against the `api_keys`
    /// table — there is no `auth.request` round-trip or JWT exchange for
    /// agent credentials. Every other bearer is treated as a JWT.
    ///
    /// The `tw_` prefix is a routing hint, not a security boundary. A
    /// malformed `tw_` string fails by hash-miss in exactly the same way a
    /// non-`tw_` string does (and surfaces the same `ApiKeyInvalidOrRevoked`
    /// error to avoid enumeration leaks).
    pub fn check_auth(&self, token: Option<&Bearer>, permissions: &[JrpcPermission]) -> Result<(), AuthError> {
        let bearer = token.ok_or(AuthError::AccessDeniedNoBearerToken)?;
        if bearer.token().starts_with(api_keys::API_KEY_PREFIX) {
            let row = api_keys::find_active_by_raw(self.wallet_sdk.store(), bearer.token())
                .map_err(AuthError::from)?
                .ok_or(AuthError::ApiKeyInvalidOrRevoked)?;
            let granted = api_keys::parse_permissions(&row.permissions).map_err(|e| {
                // A persisted row whose permissions column doesn't parse is a
                // data-integrity bug, not an authentication failure — log the
                // actual reason but surface the same opaque error so an
                // attacker can't distinguish.
                log::warn!(
                    target: "tari::ootle::walletd::auth",
                    "API key {} has unparseable permissions column: {e}",
                    row.id,
                );
                AuthError::ApiKeyInvalidOrRevoked
            })?;
            enforce_scopes(&granted, permissions)?;
            self.maybe_spawn_last_used_bump(row.id);
            return Ok(());
        }
        self.jwt_api().check_auth(Some(bearer), permissions)
    }

    /// Like `check_auth`, but rejects API-key bearers up front. Use this
    /// for endpoints that must NOT be reachable with a programmatically
    /// minted credential — currently the API-key management endpoints
    /// themselves (create / list / revoke). Restricting them to an
    /// interactive WebAuthn session means a leaked Admin API key cannot
    /// mint further keys to survive a revoke; the attacker's persistence
    /// is bounded by the lifetime of the original compromised key.
    ///
    /// The `tw_` prefix is the same routing hint used by `check_auth`;
    /// here it's the reject criterion instead of the accept criterion.
    pub fn check_auth_user_only(
        &self,
        token: Option<&Bearer>,
        permissions: &[JrpcPermission],
    ) -> Result<(), AuthError> {
        let bearer = token.ok_or(AuthError::AccessDeniedNoBearerToken)?;
        if bearer.token().starts_with(api_keys::API_KEY_PREFIX) {
            return Err(AuthError::UserAuthOnly);
        }
        self.jwt_api().check_auth(Some(bearer), permissions)
    }

    /// Spawn a `last_used_at` write only when the in-memory throttle says
    /// the previous bump for this id is outside the throttle window. Common
    /// case (busy agent, bump within window) is a single sharded DashMap
    /// read and an early return — no spawn, no DB lock, no executor wakeup.
    ///
    /// The DB-level filter on `api_key_touch_last_used` is kept as
    /// belt-and-braces for process restart, multi-daemon-against-same-DB,
    /// and the racy case where two concurrent shim calls both see a stale
    /// in-memory entry.
    fn maybe_spawn_last_used_bump(&self, id: i32) {
        let now = Instant::now();
        if let Some(prev) = self.api_key_last_used_bumps.get(&id) &&
            now.duration_since(*prev) < api_keys::LAST_USED_BUMP_THROTTLE
        {
            return;
        }
        // Update before spawning so a second call in the same window
        // short-circuits even if the spawned task hasn't run yet.
        self.api_key_last_used_bumps.insert(id, now);

        let store = self.wallet_sdk.store().clone();
        tokio::spawn(async move {
            if let Err(e) = api_keys::touch_last_used_throttled(&store, id, api_keys::LAST_USED_BUMP_THROTTLE) {
                log::warn!(
                    target: "tari::ootle::walletd::auth",
                    "Failed to bump api_keys.last_used_at for id {id}: {e}",
                );
            }
        });
    }

    pub fn jwt_api(&self) -> JwtApi<'_> {
        JwtApi::new(self.config().jwt_expiry, &self.jwt_secret)
    }

    pub fn shutdown_signal(&self) -> &ShutdownSignal {
        &self.shutdown_signal
    }

    pub fn account_monitor(&self) -> &AccountMonitorHandle {
        &self.account_monitor
    }

    pub fn transaction_service(&self) -> &TransactionServiceHandle {
        &self.transaction_service
    }

    pub fn config(&self) -> &WalletDaemonConfig {
        &self.config
    }

    pub fn authenticator(&self) -> &WalletAuthenticator {
        &self.authenticator
    }

    pub fn refresh_token_store(&self) -> &RefreshTokenStore {
        &self.refresh_token_store
    }

    pub fn webauthn_service(&self) -> Option<&WebauthnService<SqliteWalletStore>> {
        self.authenticator.webauthn_service()
    }

    pub fn webauthn(&self) -> Option<&Webauthn> {
        self.authenticator.webauthn()
    }

    /// Returns a TransactionBuilder with the current network configured.
    pub fn transaction_builder(&self) -> TransactionBuilder {
        Transaction::builder(self.config().network.as_byte())
    }
}
