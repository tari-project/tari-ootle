//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexSet;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tari_crypto::{
    keys::PublicKey as PublicKeyT,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    tari_utilities,
    tari_utilities::ByteArray,
};
use tari_engine_types::{
    hashing::{engine_hasher64, EngineHashDomainLabel},
    instruction::Instruction,
    FromByteType,
    ToByteType,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{UnsealedTransactionV1, UnsignedTransactionV1};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionSealSignature {
    public_key: RistrettoPublicKeyBytes,
    signature: SchnorrSignatureBytes,
}

impl TransactionSealSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn sign(secret_key: &RistrettoSecretKey, transaction: &UnsealedTransactionV1) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);

        let message = Self::create_message(transaction);
        Self {
            signature: RistrettoSchnorr::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys")
                .to_byte_type(),
            public_key: public_key.to_byte_type(),
        }
    }

    pub fn verify(&self, transaction: &UnsealedTransactionV1) -> bool {
        let message = Self::create_message(transaction);
        let Ok(public_key) = RistrettoPublicKey::try_from_byte_type(&self.public_key) else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::try_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }

    pub fn signature(&self) -> &SchnorrSignatureBytes {
        &self.signature
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.public_key
    }

    pub fn to_ristretto_public_key(&self) -> Result<RistrettoPublicKey, tari_utilities::ByteArrayError> {
        RistrettoPublicKey::from_canonical_bytes(self.public_key.as_bytes())
    }

    fn create_message(transaction: &UnsealedTransactionV1) -> [u8; 64] {
        engine_hasher64(EngineHashDomainLabel::TransactionSignature)
            .chain(&transaction.schema_version())
            .chain(transaction)
            .result()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionSignature {
    public_key: RistrettoPublicKeyBytes,
    signature: SchnorrSignatureBytes,
}

impl TransactionSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn sign_v1(
        secret_key: &RistrettoSecretKey,
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &UnsignedTransactionV1,
    ) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);
        let message = Self::create_message(seal_signer, transaction);

        Self {
            signature: RistrettoSchnorr::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys")
                .to_byte_type(),
            public_key: public_key.to_byte_type(),
        }
    }

    pub fn verify(&self, seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransactionV1) -> bool {
        let message = Self::create_message(seal_signer, transaction);
        let Ok(public_key) = RistrettoPublicKey::try_from_byte_type(&self.public_key) else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::try_from_byte_type(&self.signature) else {
            return false;
        };
        signature.verify(&public_key, message)
    }

    pub fn signature(&self) -> &SchnorrSignatureBytes {
        &self.signature
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.public_key
    }

    fn create_message(seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransactionV1) -> [u8; 64] {
        let signature_fields = TransactionSignatureFields::from(transaction);
        engine_hasher64(EngineHashDomainLabel::TransactionSignature)
            .chain(seal_signer)
            .chain(&signature_fields)
            .result()
    }
}

#[derive(Debug, Clone, Serialize)]
struct TransactionSignatureFields<'a> {
    network: u8,
    fee_instructions: &'a [Instruction],
    instructions: &'a [Instruction],
    inputs: &'a IndexSet<SubstateRequirement>,
    min_epoch: Option<Epoch>,
    max_epoch: Option<Epoch>,
    is_seal_signer_authorized: bool,
    dry_run: bool,
}

impl<'a> From<&'a UnsignedTransactionV1> for TransactionSignatureFields<'a> {
    fn from(transaction: &'a UnsignedTransactionV1) -> Self {
        Self {
            network: transaction.network,
            fee_instructions: &transaction.fee_instructions,
            instructions: &transaction.instructions,
            inputs: &transaction.inputs,
            min_epoch: transaction.min_epoch,
            max_epoch: transaction.max_epoch,
            is_seal_signer_authorized: transaction.is_seal_signer_authorized,
            dry_run: transaction.dry_run,
        }
    }
}
