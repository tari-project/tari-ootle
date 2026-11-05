//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::crypto::{PublicKey, Signature, SignatureDomain, SignaturePayload, SignatureScope};

use crate::intrinsics;

pub trait Verifiable {
    /// Verifies a signature bound to `scope`, besides the domain and the network.
    #[must_use]
    fn verify_in_scope(&self, scope: SignatureScope, public_key: &PublicKey, message: &[u8]) -> bool;

    /// Verifies a [`SignatureScope::Shared`] signature, which any template verifying under the same domain accepts.
    #[must_use]
    fn verify(&self, public_key: &PublicKey, message: &[u8]) -> bool {
        self.verify_in_scope(SignatureScope::Shared, public_key, message)
    }

    /// Verifies a signature made for this template only.
    #[must_use]
    fn verify_for_template(&self, public_key: &PublicKey, message: &[u8]) -> bool {
        self.verify_in_scope(SignatureScope::CurrentTemplate, public_key, message)
    }

    /// Verifies a signature made for this component only. Calling it outside a component method fails the
    /// transaction.
    #[must_use]
    fn verify_for_component(&self, public_key: &PublicKey, message: &[u8]) -> bool {
        self.verify_in_scope(SignatureScope::CurrentComponent, public_key, message)
    }

    fn assert_valid(&self, public_key: &PublicKey, message: &[u8]) {
        if !self.verify(public_key, message) {
            panic!("Signature verification failed");
        }
    }
}

impl<D: SignatureDomain> Verifiable for Signature<D> {
    fn verify_in_scope(&self, scope: SignatureScope, public_key: &PublicKey, message: &[u8]) -> bool {
        SignatureVerifier::with_domain(D::domain())
            .with_scope(scope)
            .verify(public_key, message, self.payload())
    }
}

pub struct SignatureVerifier {
    domain: &'static [u8],
    scope: SignatureScope,
}

impl SignatureVerifier {
    /// A verifier for [`SignatureScope::Shared`] signatures under `domain`.
    pub const fn with_domain(domain: &'static [u8]) -> Self {
        Self {
            domain,
            scope: SignatureScope::Shared,
        }
    }

    pub const fn with_scope(mut self, scope: SignatureScope) -> Self {
        self.scope = scope;
        self
    }
}

impl SignatureVerifier {
    #[must_use]
    pub fn verify(&self, public_key: &PublicKey, message: &[u8], payload: &SignaturePayload) -> bool {
        intrinsics::verify_signature(public_key, self.domain, self.scope, message, payload)
    }
}
