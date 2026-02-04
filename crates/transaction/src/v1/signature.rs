//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use indexmap::IndexSet;
use ootle_byte_type::{ConvertFromByteType, FromByteType, ToByteType};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tari_crypto::{
    keys::PublicKey as PublicKeyT,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    tari_utilities,
    tari_utilities::ByteArray,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement, signature::SignatureOutput};
use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{
    Instruction,
    UnsealedTransactionV1,
    UnsignedTransaction,
    UnsignedTransactionV1,
    hashing::transaction_hasher_v1,
    unsealed::UnsealedTransaction,
};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionSealSignature {
    public_key: RistrettoPublicKeyBytes,
    signature: SchnorrSignatureBytes,
}

impl TransactionSealSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn sign_v1(secret_key: &RistrettoSecretKey, transaction: &UnsealedTransactionV1) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);

        let message = Self::create_message_v1(transaction);
        Self {
            signature: RistrettoSchnorr::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys")
                .to_byte_type(),
            public_key: public_key.to_byte_type(),
        }
    }

    pub fn verify(&self, transaction: &UnsealedTransaction) -> bool {
        match transaction {
            UnsealedTransaction::V1(t) => self.verify_v1(t),
        }
    }

    pub fn verify_v1(&self, transaction: &UnsealedTransactionV1) -> bool {
        let message = Self::create_message_v1(transaction);
        let Ok(public_key) = self.public_key.try_from_byte_type() else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::convert_from_byte_type(&self.signature) else {
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

    pub fn create_message(transaction: &UnsealedTransaction) -> [u8; 64] {
        match transaction {
            UnsealedTransaction::V1(t) => Self::create_message_v1(t),
        }
    }

    pub fn create_message_v1(transaction: &UnsealedTransactionV1) -> [u8; 64] {
        transaction_hasher_v1("SealSignature")
            .chain(&transaction.schema_version())
            .chain(transaction)
            .result()
    }
}

impl From<SignatureOutput> for TransactionSealSignature {
    fn from(output: SignatureOutput) -> Self {
        Self {
            public_key: output.public_key.to_byte_type(),
            signature: output.signature.to_byte_type(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionSignature {
    public_key: RistrettoPublicKeyBytes,
    signature: SchnorrSignatureBytes,
}

impl TransactionSignature {
    pub fn new(public_key: RistrettoPublicKeyBytes, signature: SchnorrSignatureBytes) -> Self {
        Self { public_key, signature }
    }

    pub fn sign(
        secret_key: &RistrettoSecretKey,
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &UnsignedTransaction,
    ) -> Self {
        match transaction {
            UnsignedTransaction::V1(v1) => Self::sign_v1(secret_key, seal_signer, v1),
        }
    }

    pub fn sign_v1(
        secret_key: &RistrettoSecretKey,
        seal_signer: &RistrettoPublicKeyBytes,
        transaction: &UnsignedTransactionV1,
    ) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(secret_key);
        let message = Self::create_message_v1(seal_signer, transaction);

        Self {
            signature: RistrettoSchnorr::sign(secret_key, message, &mut OsRng)
                .expect("sign is infallible with Ristretto keys")
                .to_byte_type(),
            public_key: public_key.to_byte_type(),
        }
    }

    pub fn verify_v1(&self, seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransactionV1) -> bool {
        let message = Self::create_message_v1(seal_signer, transaction);
        let Ok(public_key) = self.public_key.try_from_byte_type() else {
            return false;
        };
        let Ok(signature) = RistrettoSchnorr::convert_from_byte_type(&self.signature) else {
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

    pub fn create_message(seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransaction) -> [u8; 64] {
        match transaction {
            UnsignedTransaction::V1(v1) => Self::create_message_v1(seal_signer, v1),
        }
    }

    pub fn create_message_v1(seal_signer: &RistrettoPublicKeyBytes, transaction: &UnsignedTransactionV1) -> [u8; 64] {
        let signature_fields = TransactionSignatureFields::from(transaction);
        transaction_hasher_v1("Signature")
            .chain(&transaction.schema_version())
            .chain(seal_signer)
            .chain(&signature_fields)
            .result()
    }
}

impl From<SignatureOutput> for TransactionSignature {
    fn from(output: SignatureOutput) -> Self {
        Self {
            public_key: output.public_key.to_byte_type(),
            signature: output.signature.to_byte_type(),
        }
    }
}

#[derive(Debug, Clone, borsh::BorshSerialize)]
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
