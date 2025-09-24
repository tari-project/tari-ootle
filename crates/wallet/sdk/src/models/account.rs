//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_bor::{Deserialize, Serialize};
use tari_ootle_address::OotleAddress;
use tari_template_lib::{models::ComponentAddress, prelude::RistrettoPublicKeyBytes};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Account {
    pub name: Option<String>,
    pub component_address: ComponentAddress,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub key_index: u64,
    pub is_confirmed_on_chain: bool,
    pub is_default: bool,
}

impl Account {
    pub fn address(&self) -> &ComponentAddress {
        &self.component_address
    }

    pub fn key_index(&self) -> u64 {
        self.key_index
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
    pub fn account(&self) -> &Account {
        &self.account
    }

    pub fn component_address(&self) -> &ComponentAddress {
        &self.account.component_address
    }

    pub fn address(&self) -> &OotleAddress {
        &self.address
    }

    pub fn key_index(&self) -> u64 {
        self.account.key_index
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NewAccountData {
    pub address: ComponentAddress,
}

#[derive(Debug, Clone, Default)]
pub struct AccountUpdate<'a> {
    pub name: Option<&'a str>,
    pub is_account_on_chain: Option<bool>,
}
