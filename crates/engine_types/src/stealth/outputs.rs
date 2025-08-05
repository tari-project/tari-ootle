//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey};
use tari_template_lib::{
    models::{StealthMintBalanceProof, StealthMintStatement, StealthOutputsStatement},
    types::Amount,
};

use crate::{
    crypto::{
        commit_amount,
        messages,
        range_proof::validate_bullet_proof,
        validate_elgamal_verifiable_balance_proof,
        ValidatedPrivateOutput,
    },
    hashing::EngineSchnorrSignature,
    resource_container::ResourceError,
    FromByteType,
    ToByteType,
    UtxoOutput,
};

#[derive(Debug, Clone)]
pub struct ValidatedStealthOutput {
    pub output: ValidatedPrivateOutput,
    pub owner_public_key: RistrettoPublicKey,
}

impl ValidatedStealthOutput {
    pub fn to_utxo_output(&self) -> UtxoOutput {
        UtxoOutput {
            owner_public_key: self.owner_public_key.to_byte_type(),
            output: self.output.to_private_output(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedStealthMintStatement {
    pub outputs_statement: Vec<ValidatedStealthOutput>,
    pub total_mint_amount: Amount,
    pub revealed_output_amount: Amount,
}

pub fn validate_stealth_outputs_statement(
    stmt: &StealthOutputsStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<Vec<ValidatedStealthOutput>, ResourceError> {
    if stmt.outputs.is_empty() {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "No outputs provided in the stealth statement".to_string(),
        });
    }

    validate_bullet_proof(&stmt.agg_range_proof, stmt.outputs.iter().map(|o| &o.output))?;

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
            })
        })
        .collect::<Result<_, ResourceError>>()?;

    Ok(outputs)
}

pub fn validate_stealth_mint_statement(
    stmt: &StealthMintStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthMintStatement, ResourceError> {
    validate_mint_balance_proof(&stmt.balance_proof, &stmt.outputs_statement)?;
    let outputs = validate_stealth_outputs_statement(&stmt.outputs_statement, view_key)?;
    Ok(ValidatedStealthMintStatement {
        outputs_statement: outputs,
        total_mint_amount: stmt.balance_proof.total_mint_amount,
        revealed_output_amount: stmt.outputs_statement.revealed_output_amount,
    })
}

pub fn validate_mint_balance_proof(
    balance_proof: &StealthMintBalanceProof,
    outputs_statement: &StealthOutputsStatement,
) -> Result<(), ResourceError> {
    let total_amount =
        balance_proof
            .total_mint_amount
            .non_negative_checked()
            .ok_or(ResourceError::InvalidConfidentialProof {
                details: format!(
                    "Total amount in balance proof must be non-negative but was: {}",
                    balance_proof.total_mint_amount
                ),
            })?;
    let revealed_output_amount = outputs_statement.revealed_output_amount.non_negative_checked().ok_or(
        ResourceError::InvalidConfidentialProof {
            details: format!(
                "Revealed output amount must be non-negative but was: {}",
                outputs_statement.revealed_output_amount
            ),
        },
    )?;

    let sig = EngineSchnorrSignature::try_from_byte_type(&balance_proof.excess_signature).map_err(|e| {
        ResourceError::InvalidConfidentialProof {
            details: format!("Invalid excess signature: {e}"),
        }
    })?;

    let mut commitment_sum = RistrettoPublicKey::default();
    for (i, unspent_output) in outputs_statement.outputs.iter().enumerate() {
        let commitment = PedersenCommitment::try_from_byte_type(&unspent_output.output.commitment).map_err(|_| {
            ResourceError::InvalidConfidentialProof {
                details: format!("Invalid output commitment at index {i}"),
            }
        })?;
        commitment_sum = commitment_sum + commitment.as_public_key();
    }
    let total_value_commit = commit_amount(&RistrettoSecretKey::default(), total_amount);
    let revealed_amount_commitment = commit_amount(&RistrettoSecretKey::default(), revealed_output_amount);
    let public_excess =
        commitment_sum + revealed_amount_commitment.as_public_key() - total_value_commit.as_public_key();

    eprintln!(
        "Verify: public_excess: {public_excess}, total_amount: {total_amount} nonce: {}",
        sig.get_public_nonce()
    );

    let message = messages::stealth_mint64(&public_excess, sig.get_public_nonce(), balance_proof.total_mint_amount);

    if !sig.verify_raw_uniform(&public_excess, &message) {
        return Err(ResourceError::InvalidConfidentialProof {
            details: format!(
                "Excess signature failed to validate for total amount {}",
                balance_proof.total_mint_amount
            ),
        });
    }

    Ok(())
}
