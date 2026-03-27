//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use tari_crypto::ristretto::RistrettoPublicKey;

use crate::signer;

pub trait StealthKeyPrehashSigner<S> {
    fn sign_prehash_with_stealth_key(
        &self,
        public_key: &RistrettoPublicKey,
        prehash: &[u8],
    ) -> impl Future<Output = signer::Result<S>> + Send;
}
