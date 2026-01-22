//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod private_key;

pub use private_key::*;

use crate::Address;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSigner<C> {
    address: Address,
    credentials: C,
}

impl<C> LocalSigner<C> {
    pub fn credentials(&self) -> &C {
        &self.credentials
    }
}
