//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::models::StealthOutputStatement;

use crate::{
    crypto::{range_proof::validate_bullet_proof, validate_elgamal_verifiable_balance_proof, ValidatedPrivateOutput},
    resource_container::ResourceError,
    FromByteType,
};

#[derive(Debug)]
pub struct ValidatedStealthOutputs {
    pub outputs: Vec<ValidatedPrivateOutput>,
}

pub fn validate_stealth_statement(
    stmt: &StealthOutputStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthOutputs, ResourceError> {
    if stmt.outputs.is_empty() {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "No outputs provided in the stealth statement".to_string(),
        });
    }

    validate_bullet_proof(&stmt.range_proof, &stmt.outputs)?;

    let outputs =
        stmt.outputs
            .iter()
            .map(|statement| {
                let output_commitment =
                    PedersenCommitment::try_from_byte_type(&statement.commitment).map_err(|_| {
                        ResourceError::InvalidConfidentialProof {
                            details: "Invalid commitment".to_string(),
                        }
                    })?;

                let output_public_nonce = RistrettoPublicKey::try_from_byte_type(&statement.sender_public_nonce)
                    .map_err(|_| ResourceError::InvalidConfidentialProof {
                        details: "Invalid sender public nonce".to_string(),
                    })?;

                let viewable_balance = validate_elgamal_verifiable_balance_proof(
                    &output_commitment,
                    view_key,
                    statement.viewable_balance_proof.as_ref(),
                )?;

                Ok(ValidatedPrivateOutput {
                    commitment: output_commitment,
                    stealth_public_nonce: output_public_nonce,
                    encrypted_data: statement.encrypted_data.clone(),
                    minimum_value_promise: statement.minimum_value_promise,
                    viewable_balance,
                })
            })
            .collect::<Result<_, ResourceError>>()?;

    Ok(ValidatedStealthOutputs { outputs })
}
