//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexSet;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tari_common_types::types::{PublicKey, Signature};
use tari_crypto::{
    keys::PublicKey as PublicKeyT,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_dan_common_types::{Epoch, SubstateRequirement};
use tari_engine_types::{
    hashing::{hasher64, EngineHashDomainLabel},
    instruction::Instruction,
};

use crate::{UnsealedTransactionV1, UnsignedTransactionV1};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct TransactionSealSignature {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    public_key: PublicKey,
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce: string, signature: string}"))]
    signature: Signature,
}

impl TransactionSealSignature {
    pub fn new(public_key: PublicKey, signature: Signature) -> Self {
        Self { public_key, signature }
    }

    pub fn sign(secret_key: &RistrettoSecretKey, transaction: &UnsealedTransactionV1) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);

        let message = Self::create_message(transaction);
        Self {
            signature: Signature::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys"),
            public_key,
        }
    }

    pub fn verify(&self, transaction: &UnsealedTransactionV1) -> bool {
        let message = Self::create_message(transaction);
        self.signature.verify(&self.public_key, message)
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    fn create_message(transaction: &UnsealedTransactionV1) -> [u8; 64] {
        hasher64(EngineHashDomainLabel::TransactionSignature)
            .chain(&transaction.schema_version())
            .chain(transaction)
            .result()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct TransactionSignature {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    public_key: PublicKey,
    #[cfg_attr(feature = "ts", ts(type = "{public_nonce: string, signature: string}"))]
    signature: Signature,
}

impl TransactionSignature {
    pub fn new(public_key: PublicKey, signature: Signature) -> Self {
        Self { public_key, signature }
    }

    pub fn sign_v1(
        secret_key: &RistrettoSecretKey,
        seal_signer: &PublicKey,
        transaction: &UnsignedTransactionV1,
    ) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);
        let message = Self::create_message(seal_signer, transaction);

        Self {
            signature: Signature::sign(secret_key, message, &mut OsRng).unwrap(),
            public_key,
        }
    }

    pub fn verify(&self, seal_signer: &PublicKey, transaction: &UnsignedTransactionV1) -> bool {
        let message = Self::create_message(seal_signer, transaction);
        self.signature.verify(&self.public_key, message)
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    fn create_message(seal_signer: &PublicKey, transaction: &UnsignedTransactionV1) -> [u8; 64] {
        let signature_fields = TransactionSignatureFields::from(transaction);
        hasher64(EngineHashDomainLabel::TransactionSignature)
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
        }
    }
}
