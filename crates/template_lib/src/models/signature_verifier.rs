//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::{EngineOp, call_engine};
use tari_template_lib_types::{
    crypto::{PublicKey, Signature, SignatureDomain, SignaturePayload},
    engine_args::{SignatureAction, SignatureInvokeArg, SignatureVerifyArgRef},
};

use crate::args::InvokeResult;

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
        let resp: InvokeResult = call_engine(EngineOp::SignatureInvoke, &SignatureInvokeArg {
            action: SignatureAction::Verify,
            args: invoke_args![SignatureVerifyArgRef {
                public_key,
                domain: self.domain,
                message,
                payload,
            }],
        });

        resp.decode().expect("Failed to decode signature verification result")
    }
}
