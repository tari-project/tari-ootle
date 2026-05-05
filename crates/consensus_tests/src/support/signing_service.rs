//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::PrivateKey;
use tari_consensus::traits::{ValidatorSignatureVerifierService, ValidatorSignerService};
use tari_consensus_types::{ToSignatureMessage, ValidatorSchnorrSignature};
use tari_crypto::ristretto::RistrettoPublicKey;

use super::{TestAddress, helpers};

#[derive(Debug, Clone)]
pub struct TestVoteSignatureService {
    pub public_key: RistrettoPublicKey,
    pub secret_key: PrivateKey,
}

impl TestVoteSignatureService {
    pub fn new(addr: TestAddress) -> Self {
        let (secret_key, public_key) = helpers::derive_keypair_from_address(&addr);
        Self { public_key, secret_key }
    }
}

impl ValidatorSignerService for TestVoteSignatureService {
    fn sign<M: ToSignatureMessage>(&self, message: &M) -> ValidatorSchnorrSignature {
        ValidatorSchnorrSignature::sign(&self.secret_key, message.to_signature_message(), &mut rand::rng()).unwrap()
    }

    fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }
}

impl ValidatorSignatureVerifierService for TestVoteSignatureService {}
