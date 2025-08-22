//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_template_lib::{models::ComponentAddress, prelude::RistrettoPublicKeyBytes};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Account {
    pub name: Option<String>,
    pub address: ComponentAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub key_index: u64,
    pub is_confirmed_on_chain: bool,
    pub is_default: bool,
}

impl Display for Account {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.name {
            Some(ref name) => write!(f, "{} ({})", name, self.address),
            None => write!(f, "{}", self.address),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AccountWithPublicKey {
    pub account: Account,
    pub owner_public_key: RistrettoPublicKeyBytes,
}

impl AccountWithPublicKey {
    pub fn account(&self) -> &Account {
        &self.account
    }

    pub fn address(&self) -> &ComponentAddress {
        &self.account.address
    }

    pub fn key_index(&self) -> u64 {
        self.account.key_index
    }

    pub fn owner_public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.owner_public_key
    }

    pub fn is_confirmed_on_chain(&self) -> bool {
        self.account.is_confirmed_on_chain
    }
}

impl Display for AccountWithPublicKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (pubkey: {})", self.account, self.owner_public_key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NewAccountData {
    pub address: ComponentAddress,
}

#[derive(Debug, Clone, Default)]
pub struct AccountUpdate {
    pub name: Option<String>,
    pub is_account_on_chain: Option<bool>,
}
