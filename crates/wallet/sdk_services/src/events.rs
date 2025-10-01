//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::commit_result::FinalizeResult;
use tari_ootle_wallet_sdk::models::{Account, NewAccountData, TransactionStatus};
use tari_template_lib::prelude::ComponentAddress;
use tari_transaction::TransactionId;

#[derive(Debug, Clone)]
pub enum WalletEvent {
    TransactionSubmitted(TransactionSubmittedEvent),
    TransactionFinalized(TransactionFinalizedEvent),
    TransactionInvalid(TransactionInvalidEvent),
    AccountCreatedOnChain(AccountCreatedEvent),
    AccountChangedOnChain(AccountChangedEvent),
    AuthLoginRequest(#[allow(dead_code)] AuthLoginRequestEvent),
}

impl From<TransactionSubmittedEvent> for WalletEvent {
    fn from(value: TransactionSubmittedEvent) -> Self {
        Self::TransactionSubmitted(value)
    }
}

impl From<TransactionFinalizedEvent> for WalletEvent {
    fn from(value: TransactionFinalizedEvent) -> Self {
        Self::TransactionFinalized(value)
    }
}

impl From<AccountChangedEvent> for WalletEvent {
    fn from(value: AccountChangedEvent) -> Self {
        Self::AccountChangedOnChain(value)
    }
}

impl From<TransactionInvalidEvent> for WalletEvent {
    fn from(value: TransactionInvalidEvent) -> Self {
        Self::TransactionInvalid(value)
    }
}

impl From<AuthLoginRequestEvent> for WalletEvent {
    fn from(value: AuthLoginRequestEvent) -> Self {
        Self::AuthLoginRequest(value)
    }
}

impl From<AccountCreatedEvent> for WalletEvent {
    fn from(value: AccountCreatedEvent) -> Self {
        Self::AccountCreatedOnChain(value)
    }
}

#[derive(Debug, Clone)]
pub struct TransactionSubmittedEvent {
    pub transaction_id: TransactionId,
    /// Set to Some if this transaction results in a new account
    pub new_account: Option<NewAccountData>,
}

#[derive(Debug, Clone)]
pub struct TransactionFinalizedEvent {
    pub transaction_id: TransactionId,
    pub finalize: FinalizeResult,
    pub final_fee: u64,
    pub status: TransactionStatus,
}

#[derive(Debug, Clone)]
pub struct AccountCreatedEvent {
    pub account: Account,
    pub _created_by_tx: TransactionId,
}

#[derive(Debug, Clone)]
pub struct AccountChangedEvent {
    pub account_address: ComponentAddress,
}

#[derive(Debug, Clone)]
pub struct TransactionInvalidEvent {
    pub transaction_id: TransactionId,
    pub status: TransactionStatus,
    pub finalize: Option<FinalizeResult>,
    pub final_fee: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AuthLoginRequestEvent;
