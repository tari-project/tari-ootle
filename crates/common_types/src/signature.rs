//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSchnorr};

#[derive(Debug, Clone)]
pub struct SignatureOutput {
    pub signature: RistrettoSchnorr,
    pub public_key: RistrettoPublicKey,
}
