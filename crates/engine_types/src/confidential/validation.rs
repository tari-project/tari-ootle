//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::{ConvertFromByteType, FromByteType};
use tari_crypto::{
    ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment},
    tari_utilities::ByteArray,
};
use tari_template_lib::types::{Amount, confidential::ConfidentialOutputStatement};

use crate::{
    crypto::{ValidateOutputBody, range_proof::validate_bullet_proof, validate_elgamal_verifiable_balance_proof},
    resource_container::ResourceError,
};

#[derive(Debug)]
pub struct ValidatedConfidentialProof {
    pub output: Option<ValidateOutputBody>,
    pub change_output: Option<ValidateOutputBody>,
    pub output_revealed_amount: Amount,
    pub change_revealed_amount: Amount,
}

pub fn validate_confidential_statement(
    proof: &ConfidentialOutputStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedConfidentialProof, ResourceError> {
    if proof.output_revealed_amount.is_negative() || proof.change_revealed_amount.is_negative() {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "Revealed amounts must be positive".to_string(),
        });
    }

    let maybe_output = proof
        .output
        .as_ref()
        .map(|statement| {
            let output_commitment =
                PedersenCommitment::convert_from_byte_type(&statement.commitment).map_err(|_| {
                    ResourceError::InvalidConfidentialProof {
                        details: "Invalid commitment".to_string(),
                    }
                })?;

            let output_public_nonce = statement.sender_public_nonce.try_from_byte_type().map_err(|_| {
                ResourceError::InvalidConfidentialProof {
                    details: "Invalid sender public nonce".to_string(),
                }
            })?;

            let viewable_balance = validate_elgamal_verifiable_balance_proof(
                &output_commitment,
                view_key,
                statement.viewable_balance_proof.as_ref(),
            )?;

            Ok::<_, ResourceError>(ValidateOutputBody {
                commitment: output_commitment,
                public_nonce: output_public_nonce,
                encrypted_data: statement.encrypted_data.clone(),
                minimum_value_promise: statement.minimum_value_promise,
                viewable_balance,
            })
        })
        .transpose()?;

    let maybe_change = proof
        .change_statement
        .as_ref()
        .map(|stmt| {
            let commitment = PedersenCommitment::from_canonical_bytes(&*stmt.commitment).map_err(|_| {
                ResourceError::InvalidConfidentialProof {
                    details: "Invalid commitment".to_string(),
                }
            })?;

            let stealth_public_nonce =
                RistrettoPublicKey::from_canonical_bytes(&*stmt.sender_public_nonce).map_err(|_| {
                    ResourceError::InvalidConfidentialProof {
                        details: "Invalid sender public nonce".to_string(),
                    }
                })?;

            let viewable_balance =
                validate_elgamal_verifiable_balance_proof(&commitment, view_key, stmt.viewable_balance_proof.as_ref())?;

            Ok(ValidateOutputBody {
                commitment,
                public_nonce: stealth_public_nonce,
                encrypted_data: stmt.encrypted_data.clone(),
                minimum_value_promise: stmt.minimum_value_promise,
                viewable_balance,
            })
        })
        .transpose()?;

    if maybe_output.is_none() && maybe_change.is_none() {
        if !proof.range_proof.is_empty() {
            return Err(ResourceError::InvalidConfidentialProof {
                details: "Range proof is invalid because it was provided (non-empty) but the proof contained no \
                          confidential outputs"
                    .to_string(),
            });
        }
    } else {
        validate_bullet_proof(
            &proof.range_proof,
            proof.output.as_ref().into_iter().chain(proof.change_statement.as_ref()),
        )?;
    }

    Ok(ValidatedConfidentialProof {
        output: maybe_output,
        change_output: maybe_change,
        output_revealed_amount: proof.output_revealed_amount,
        change_revealed_amount: proof.change_revealed_amount,
    })
}
