//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use tari_template_lib_types::{crypto::RistrettoPublicKeyBytes, OwnerRule};

/// Data that is needed to represent ownership of a value (resource or component method).
/// Owners are the only ones allowed to update the values's access rules after creation
#[derive(Debug, Clone)]
pub struct Ownership<'a> {
    pub owner_key: Option<&'a RistrettoPublicKeyBytes>,
    pub owner_rule: Cow<'a, OwnerRule>,
}
