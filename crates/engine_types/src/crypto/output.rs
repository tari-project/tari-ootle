//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use ootle_byte_type::{ConvertFromByteType, ToByteType};
use tari_bor::{Deserialize, Serialize};
use tari_crypto::{
    extended_range_proof::Statement,
    ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment},
};
use tari_template_lib::types::{EncryptedData, crypto::RistrettoPublicKeyBytes, stealth::UnspentOutput};

use crate::{
    crypto::{
        ElgamalVerifiableBalance,
        ElgamalVerifiableBalanceBytes,
        check_viewable_balance_proof_presence,
        validate_elgamal_verifiable_balance_proof,
    },
    resource_container::ResourceError,
};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OutputBody {
    #[n(0)]
    pub public_nonce: RistrettoPublicKeyBytes,
    #[n(1)]
    pub encrypted_data: EncryptedData,
    #[n(2)]
    #[cfg_attr(feature = "ts", ts(type = "number | bigint"))]
    pub minimum_value_promise: u64,
    #[n(3)]
    pub viewable_balance: Option<ElgamalVerifiableBalanceBytes>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateOutputBody {
    pub commitment: PedersenCommitment,
    pub public_nonce: RistrettoPublicKey,
    pub encrypted_data: EncryptedData,
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<ElgamalVerifiableBalance>,
}

impl ValidateOutputBody {
    pub fn into_output_body(self) -> OutputBody {
        OutputBody {
            public_nonce: self.public_nonce.to_byte_type(),
            encrypted_data: self.encrypted_data,
            minimum_value_promise: self.minimum_value_promise,
            viewable_balance: self.viewable_balance.as_ref().map(|b| b.to_byte_type()),
        }
    }
}

/// An output whose points are decompressed and whose viewable balance proof is present exactly when the resource has a
/// view key. Its proofs are unverified: callers decode every output of a statement before verifying any proof, so a
/// malformed statement is rejected before the first scalar multiplication.
pub(crate) struct DecodedOutput<'a> {
    output: &'a UnspentOutput,
    commitment: PedersenCommitment,
    public_nonce: RistrettoPublicKey,
}

impl<'a> DecodedOutput<'a> {
    pub(crate) fn decode(
        output: &'a UnspentOutput,
        view_key: Option<&RistrettoPublicKey>,
    ) -> Result<Self, ResourceError> {
        let commitment = PedersenCommitment::convert_from_byte_type(&output.commitment).map_err(|_| {
            ResourceError::InvalidConfidentialProof {
                details: "Invalid commitment".to_string(),
            }
        })?;
        let public_nonce = RistrettoPublicKey::convert_from_byte_type(&output.sender_public_nonce).map_err(|_| {
            ResourceError::InvalidConfidentialProof {
                details: "Invalid sender public nonce".to_string(),
            }
        })?;
        check_viewable_balance_proof_presence(view_key, output.viewable_balance_proof.as_ref())?;
        Ok(Self {
            output,
            commitment,
            public_nonce,
        })
    }

    pub(crate) fn range_proof_statement(&self) -> Statement<RistrettoPublicKey> {
        Statement {
            commitment: self.commitment.clone(),
            minimum_value_promise: self.output.minimum_value_promise,
        }
    }

    /// Verifies the viewable balance proof. The range proof covers a whole statement and is verified by the caller.
    pub(crate) fn verify_viewable_balance(
        self,
        view_key: Option<&RistrettoPublicKey>,
    ) -> Result<ValidateOutputBody, ResourceError> {
        let viewable_balance = validate_elgamal_verifiable_balance_proof(
            &self.commitment,
            view_key,
            self.output.viewable_balance_proof.as_ref(),
        )?;
        Ok(ValidateOutputBody {
            commitment: self.commitment,
            public_nonce: self.public_nonce,
            encrypted_data: self.output.encrypted_data.clone(),
            minimum_value_promise: self.output.minimum_value_promise,
            viewable_balance,
        })
    }
}
