//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};

use crate::key_provider::error::KeyProviderError;

pub type Result<T> = std::result::Result<T, KeyProviderError>;

/// Provider of random output masks (blinding factors) for Pedersen commitments.
#[async_trait]
pub trait OutputMaskProvider {
    async fn next_mask(&self) -> Result<RistrettoSecretKey>;
}

/// Provider of Diffie-Hellman KDF-derived secret keys for stealth address derivation.
#[async_trait]
pub trait DiffieHellmanKdfKeyProvider<H> {
    async fn create_kdf_dh_key(&self, hasher: H, public_key: &RistrettoPublicKey) -> Result<RistrettoSecretKey>;
}
