//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::engine_types::component::derive_component_address_from_public_key;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib_types::ComponentAddress;

/// An Ootle network address, identifying an account by its public key and network.
///
/// Create addresses using the [`address!`](crate::address) macro:
///
/// ```rust,ignore
/// let addr = address!("otl_loc_10mc0v2lyy43kldl...");
/// ```
pub type Address = tari_ootle_address::OotleAddress;

/// Derives the on-chain component address for an account from its [`Address`].
pub trait ToAccountAddress {
    fn to_account_address(&self) -> ComponentAddress;
}

impl ToAccountAddress for Address {
    fn to_account_address(&self) -> ComponentAddress {
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, self.account_public_key())
    }
}
