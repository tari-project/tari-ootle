//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tari_common_types::types::PrivateKey;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey, signatures::SchnorrSignature};
use tari_engine_types::{FromByteType, ToByteType};
use tari_hashing::ValidatorNodeHashDomain;
use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

pub type ValidatorSchnorrSignature = SchnorrSignature<RistrettoPublicKey, PrivateKey, ValidatorNodeHashDomain>;

#[derive(Clone, Debug, Hash, Deserialize, Serialize, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ValidatorSignature {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub public_key: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce : string, signature: string}"))]
    pub signature: SchnorrSignatureBytes,
}

impl ValidatorSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.public_key
    }
}

impl ValidatorSignature {
    pub fn sign<M: AsRef<[u8]>>(secret_key: &PrivateKey, message: M) -> Self {
        let signature =
            ValidatorSchnorrSignature::sign(secret_key, message, &mut OsRng).expect("sign_message is infallible");
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);
        Self::new(public_key.to_byte_type(), signature.to_byte_type())
    }

    pub fn verify<M: AsRef<[u8]>>(&self, message: M) -> bool {
        let Ok(public_key) = RistrettoPublicKey::try_from_byte_type(&self.public_key) else {
            return false;
        };
        let Ok(signature) = ValidatorSchnorrSignature::try_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }
}
