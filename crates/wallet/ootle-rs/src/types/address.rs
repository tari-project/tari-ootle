//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{apis::accounts::derive_account_address_from_public_key, OotleAddress};
use tari_template_lib_types::ComponentAddress;

pub type Address = OotleAddress;

pub trait ToAccountAddress {
    fn to_account_address(&self) -> ComponentAddress;
}

impl ToAccountAddress for Address {
    fn to_account_address(&self) -> ComponentAddress {
        derive_account_address_from_public_key(self.account_public_key())
    }
}
