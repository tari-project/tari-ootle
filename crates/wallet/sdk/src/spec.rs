//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{key_managers::WalletKeyStore, network::WalletNetworkInterface, storage::WalletStore};

pub trait WalletSdkSpec {
    type Store: WalletStore;
    type KeyStore: WalletKeyStore;
    type NetworkInterface: WalletNetworkInterface;
}

// Allow: (previously warn) this is a known limitation of the type checker that may be lifted in a future edition.
//         see issue #112792 <https://github.com/rust-lang/rust/issues/112792> for more information

#[allow(type_alias_bounds)]
pub type KeyStoreError<TSpec: WalletSdkSpec> = <TSpec::KeyStore as WalletKeyStore>::Error;
#[allow(type_alias_bounds)]
pub type NetworkInterfaceError<TSpec: WalletSdkSpec> = <TSpec::NetworkInterface as WalletNetworkInterface>::Error;
