//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::crypto::{PublicKey, Signature, SignatureDomain, SignaturePayload};

use crate::intrinsics;

pub trait Verifiable {
    fn verify(&self, public_key: &PublicKey, message: &[u8]) -> bool;

    fn assert_valid(&self, public_key: &PublicKey, message: &[u8]) {
        if !self.verify(public_key, message) {
            panic!("Signature verification failed");
        }
    }
}

impl<D: SignatureDomain> Verifiable for Signature<D> {
    fn verify(&self, public_key: &PublicKey, message: &[u8]) -> bool {
        SignatureVerifier::with_domain(D::domain()).verify(public_key, message, self.payload())
    }
}
pub struct SignatureVerifier {
    domain: &'static [u8],
}

impl SignatureVerifier {
    pub const fn with_domain(domain: &'static [u8]) -> Self {
        Self { domain }
    }
}

impl SignatureVerifier {
    pub fn verify(&self, public_key: &PublicKey, message: &[u8], payload: &SignaturePayload) -> bool {
        intrinsics::schnorr_verify_with_domain(public_key, self.domain, message, payload)
    }
}
