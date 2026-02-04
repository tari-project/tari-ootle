//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::component::derive_component_address_from_public_key;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{ComponentAddress, crypto::RistrettoPublicKeyBytes};

pub fn derive_account_address_from_public_key(public_key: &RistrettoPublicKeyBytes) -> ComponentAddress {
    derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, public_key)
}
