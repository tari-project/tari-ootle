//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::{models::StealthOutputsStatement, types::crypto::UtxoTag};

use crate::{
    crypto::{range_proof::validate_bullet_proof, validate_elgamal_verifiable_balance_proof, ValidatedPrivateOutput},
    resource_container::ResourceError,
    FromByteType,
    ToByteType,
    UtxoOutput,
};

#[derive(Debug, Clone)]
pub struct ValidatedStealthOutput {
    pub output: ValidatedPrivateOutput,
    pub owner_public_key: RistrettoPublicKey,
    pub tag: UtxoTag,
}

impl ValidatedStealthOutput {
    pub fn to_utxo_output(&self) -> UtxoOutput {
        UtxoOutput {
            owner_public_key: self.owner_public_key.to_byte_type(),
            output: self.output.to_private_output(),
            tag: self.tag,
        }
    }
}

pub fn validate_stealth_outputs_statement(
    stmt: &StealthOutputsStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<Vec<ValidatedStealthOutput>, ResourceError> {
    // Edge case: Asserts that the bulletproof is 0 bytes if there are no outputs
    validate_bullet_proof(&stmt.agg_range_proof, stmt.outputs.iter().map(|o| &o.output))?;
    if stmt.outputs.is_empty() {
        return Ok(vec![]);
    }

    let outputs = stmt
        .outputs
        .iter()
        .map(|statement| {
            let output = &statement.output;
            let output_commitment = PedersenCommitment::try_from_byte_type(&output.commitment).map_err(|_| {
                ResourceError::InvalidConfidentialProof {
                    details: "Invalid commitment".to_string(),
                }
            })?;

            let output_public_nonce =
                RistrettoPublicKey::try_from_byte_type(&output.sender_public_nonce).map_err(|_| {
                    ResourceError::InvalidConfidentialProof {
                        details: "Invalid sender public nonce".to_string(),
                    }
                })?;

            let viewable_balance = validate_elgamal_verifiable_balance_proof(
                &output_commitment,
                view_key,
                output.viewable_balance_proof.as_ref(),
            )?;
            let output = ValidatedPrivateOutput {
                commitment: output_commitment,
                public_nonce: output_public_nonce,
                encrypted_data: output.encrypted_data.clone(),
                minimum_value_promise: output.minimum_value_promise,
                viewable_balance,
            };

            Ok(ValidatedStealthOutput {
                output,
                owner_public_key: RistrettoPublicKey::try_from_byte_type(&statement.owner_public_key).map_err(
                    |_| ResourceError::InvalidConfidentialProof {
                        details: "Invalid owner public key".to_string(),
                    },
                )?,
                tag: statement.tag,
            })
        })
        .collect::<Result<_, ResourceError>>()?;

    Ok(outputs)
}
