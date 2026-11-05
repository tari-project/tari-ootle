//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! sr25519 signatures. Transactions sign under `tari_ootle_transaction::TRANSACTION_SIGNING_CONTEXT`.

use tari_crypto::ristretto::RistrettoPublicKey;
pub use tari_ootle_sr25519::{SigningContext, Sr25519SignatureBytes, components, from_components};

#[derive(Debug, Clone)]
pub struct SignatureOutput {
    pub signature: Sr25519SignatureBytes,
    pub public_key: RistrettoPublicKey,
}
