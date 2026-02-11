//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{IntoSigned, Signable, Transaction, TransactionSealSignature, TransactionSignature, UnsealedTransactionV1};

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UnsealedTransaction {
    V1(UnsealedTransactionV1),
}

impl UnsealedTransaction {
    pub fn schema_version(&self) -> u16 {
        match self {
            Self::V1(t) => t.schema_version(),
        }
    }

    pub fn seal(self, secret: &RistrettoSecretKey) -> Transaction {
        match self {
            Self::V1(t) => t.seal(secret),
        }
    }

    pub fn seal_with_signature(self, signature: TransactionSealSignature) -> Transaction {
        match self {
            Self::V1(t) => t.seal_with_signature(signature),
        }
    }

    pub fn add_signer(self, seal_signer: &RistrettoPublicKeyBytes, secret: &RistrettoSecretKey) -> Self {
        match self {
            Self::V1(t) => t.add_signer(seal_signer, secret).into(),
        }
    }

    pub fn is_dry_run(&self) -> bool {
        match self {
            Self::V1(t) => t.is_dry_run(),
        }
    }

    pub fn verify_all_signatures(&self, seal_signer: &RistrettoPublicKeyBytes) -> bool {
        match self {
            Self::V1(t) => t.verify_all_signatures(seal_signer),
        }
    }

    pub fn add_signature(self, signature: TransactionSignature) -> Self {
        match self {
            Self::V1(t) => t.add_signature(signature).into(),
        }
    }

    fn with_seal_signature(self, signature: TransactionSealSignature) -> Transaction {
        Transaction::new(self, signature)
    }
}

impl From<UnsealedTransactionV1> for UnsealedTransaction {
    fn from(value: UnsealedTransactionV1) -> Self {
        Self::V1(value)
    }
}

impl Signable for UnsealedTransaction {
    type MessageOutput = [u8; 64];
    type Signature = TransactionSealSignature;

    fn to_signing_message(&self, _context: ()) -> Self::MessageOutput {
        TransactionSealSignature::create_message(self)
    }
}

impl Signable<&RistrettoPublicKeyBytes> for UnsealedTransaction {
    type MessageOutput = [u8; 64];
    type Signature = TransactionSignature;

    fn to_signing_message(&self, context: &RistrettoPublicKeyBytes) -> Self::MessageOutput {
        match self {
            Self::V1(t) => t.to_signing_message(context),
        }
    }
}

impl IntoSigned for UnsealedTransaction {
    type SignedOutput = Transaction;

    fn into_signed(self, sig: TransactionSealSignature) -> Self::SignedOutput {
        self.with_seal_signature(sig)
    }
}

impl IntoSigned<&RistrettoPublicKeyBytes> for UnsealedTransaction {
    type SignedOutput = Self;

    fn into_signed(self, sig: TransactionSignature) -> Self::SignedOutput {
        self.add_signature(sig)
    }
}
