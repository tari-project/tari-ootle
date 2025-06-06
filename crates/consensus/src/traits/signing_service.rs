//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{SignedMessage, ToSignatureMessage, ValidatorSchnorrSignature};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::FromByteType;

const LOG_TARGET: &str = "tari::consensus::signer_service";

pub trait ValidatorSignerService {
    fn sign<M: ToSignatureMessage>(&self, message: &M) -> ValidatorSchnorrSignature;

    fn public_key(&self) -> &RistrettoPublicKey;
}

pub trait ValidatorSignatureVerifierService {
    fn verify<M: SignedMessage>(&self, message: &M) -> bool {
        let Ok(public_key) = RistrettoPublicKey::try_from_byte_type(message.public_key()) else {
            warn!(
                target: LOG_TARGET,
                "Malformed signature public key. Raw: {}",
                message.public_key()
            );
            return false;
        };
        let Ok(signature) = ValidatorSchnorrSignature::try_from_byte_type(message.signature()) else {
            warn!(
                target: LOG_TARGET,
                "Malformed signature. Raw: {}",
                message.signature()
            );
            return false;
        };
        let message = message.to_signature_message();
        signature.verify(&public_key, message)
    }
}
