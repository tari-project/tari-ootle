//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_ootle_address::OotleAddress;
use tari_ootle_common_types::Epoch;
use tari_template_lib::types::{ComponentAddress, crypto::RistrettoPublicKeyBytes};

use crate::models::KeyId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Account {
    pub name: Option<String>,
    pub component_address: ComponentAddress,
    pub view_only_key_id: KeyId,
    pub owner_key_id: Option<KeyId>,
    pub owner_public_key: RistrettoPublicKeyBytes,
    pub birthday_epoch: Epoch,
    pub is_confirmed_on_chain: bool,
    pub is_default: bool,
}

impl Account {
    pub fn component_address(&self) -> &ComponentAddress {
        &self.component_address
    }

    pub fn view_only_key_id(&self) -> KeyId {
        self.view_only_key_id
    }

    pub fn owner_key_id(&self) -> Option<KeyId> {
        self.owner_key_id
    }

    pub fn owner_public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.owner_public_key
    }

    pub fn birthday_epoch(&self) -> Epoch {
        self.birthday_epoch
    }

    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }

    pub fn is_confirmed_on_chain(&self) -> bool {
        self.is_confirmed_on_chain
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }
}

impl Display for Account {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.name {
            Some(ref name) => write!(f, "{} ({})", name, self.component_address),
            None => write!(f, "{}", self.component_address),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AccountWithAddress {
    pub account: Account,
    pub address: OotleAddress,
}

impl AccountWithAddress {
    pub fn new(account: Account, address: OotleAddress) -> Self {
        Self { account, address }
    }

    pub fn birthday_epoch(&self) -> Epoch {
        self.account.birthday_epoch()
    }

    pub fn account(&self) -> &Account {
        &self.account
    }

    pub fn component_address(&self) -> &ComponentAddress {
        &self.account.component_address
    }

    pub fn address(&self) -> &OotleAddress {
        &self.address
    }

    pub fn name(&self) -> Option<&String> {
        self.account.name.as_ref()
    }

    pub fn view_only_key_id(&self) -> KeyId {
        self.account.view_only_key_id
    }

    pub fn owner_key_id(&self) -> Option<KeyId> {
        self.account.owner_key_id
    }

    pub fn owner_public_key(&self) -> &RistrettoPublicKeyBytes {
        self.address.account_public_key()
    }

    pub fn view_only_public_key(&self) -> &RistrettoPublicKeyBytes {
        self.address.view_only_key()
    }

    pub fn is_confirmed_on_chain(&self) -> bool {
        self.account.is_confirmed_on_chain
    }
}

impl Display for AccountWithAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.account, self.address)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub struct NewAccountData {
    pub address: ComponentAddress,
}

/// Extra context attached to a transaction at submission time and broadcast to transaction monitors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionContext {
    /// Wallet account(s) this transaction involves. Persisted at insert so the transaction list can be
    /// filtered per account. Accounts not owned by this wallet are ignored when linking.
    pub linked_accounts: Vec<ComponentAddress>,
    /// Additional typed context consumed by transaction monitors (e.g. new-account / claim-burn flows).
    pub kind: Option<TransactionContextKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionContextKind {
    NewAccount(NewAccountData),
    ClaimBurn { file_name: String },
}

impl TransactionContext {
    /// Context linking a transaction to the given wallet account(s), with no monitor-specific kind.
    pub fn with_accounts<I: IntoIterator<Item = ComponentAddress>>(accounts: I) -> Self {
        Self {
            linked_accounts: accounts.into_iter().collect(),
            kind: None,
        }
    }

    /// Sets the monitor-specific kind, returning self for chaining.
    pub fn with_kind(mut self, kind: TransactionContextKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn new_account_data(&self) -> Option<&NewAccountData> {
        match &self.kind {
            Some(TransactionContextKind::NewAccount(data)) => Some(data),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AccountUpdate<'a> {
    pub name: Option<&'a str>,
    pub is_account_on_chain: Option<bool>,
}
