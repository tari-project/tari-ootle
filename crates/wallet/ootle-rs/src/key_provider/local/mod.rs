//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod generic_impls;
mod private_key;

pub use private_key::*;

use crate::Address;

/// A key provider backed by local credentials.
///
/// Parameterized by the credential type `C`. The most common instantiation is
/// [`PrivateKeyProvider`] (i.e. `LocalKeyProvider<OotleSecretKey>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalKeyProvider<C> {
    address: Address,
    credentials: C,
}

impl<C> LocalKeyProvider<C> {
    pub fn credentials(&self) -> &C {
        &self.credentials
    }
}
