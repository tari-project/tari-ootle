//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArrayError,
};
use tari_template_lib::{models::StealthTransferStatement, types::Amount};

use crate::{
    crypto::{commit_amount_checked, messages, try_decode_to_signature},
    resource_container::ResourceError,
    stealth,
    stealth::ValidatedStealthOutput,
    ConvertFromByteType,
};

const LOG_TARGET: &str = "tari::engine_types::stealth::transfer";

#[derive(Debug, Clone)]
pub struct ValidatedStealthTransfer {
    pub outputs: Vec<ValidatedStealthOutput>,
    pub revealed_output_amount: Amount,
}

pub fn validate_transfer(
    transfer: &StealthTransferStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthTransfer, ResourceError> {
    basic_validations(transfer)?;
    let validated_outputs = stealth::validate_stealth_outputs_statement(&transfer.outputs_statement, view_key)?;
    if transfer.balance_proof.public_nonce().is_zero() {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Balance proof public nonce cannot be zero".to_string(),
        });
    }

    let balance_proof =
        try_decode_to_signature(&transfer.balance_proof).ok_or_else(|| ResourceError::InvalidBalanceProof {
            details: "Malformed balance proof".to_string(),
        })?;

    // EDGE CASE: If there are no inputs and no outputs, the public excess will be 0.G
    if transfer.inputs_statement.inputs.is_empty() && validated_outputs.is_empty() {
        // Ensure that the range proof is empty
        if !transfer.outputs_statement.agg_range_proof.is_empty() {
            return Err(ResourceError::InvalidBalanceProof {
                details: "Range proof must be empty when there are no inputs or outputs".to_string(),
            });
        }

        // The public excess is 0 (s = r + e.0) so we check s.G ?= r.G + e.0.G - NOTE: normal signature verification
        // explicitly rejects the zero key however since we've checked that the revealed amounts are equal and
        // there are no inputs or outputs (basic_validations), we consider this a valid balance proof.
        if RistrettoPublicKey::from_secret_key(balance_proof.get_signature()) != *balance_proof.get_public_nonce() {
            return Err(ResourceError::InvalidBalanceProof {
                details: "Balance proof signature verification failed for revealed amount. This typically indicates \
                          that the transfer statement total input amount != total output amount."
                    .to_string(),
            });
        }

        return Ok(ValidatedStealthTransfer {
            outputs: vec![],
            revealed_output_amount: transfer.outputs_statement.revealed_output_amount,
        });
    }

    let agg_outputs = validated_outputs.iter().fold(RistrettoPublicKey::default(), |acc, o| {
        acc + o.output.commitment.as_public_key()
    });

    let agg_inputs = transfer
        .inputs_statement
        .inputs
        .iter()
        .try_fold(RistrettoPublicKey::default(), |sum, input| {
            let commit = PedersenCommitment::convert_from_byte_type(&input.commitment)?;
            Ok::<_, ByteArrayError>(sum + commit.as_public_key())
        })
        .map_err(|e| ResourceError::InvalidConfidentialProof {
            details: format!("Malformed commitment in transfer inputs: {e}"),
        })?;

    // We assume that the input amount is available and only check that the maths is correct. The engine is responsible
    // for checking that the input amount is actually available.
    let revealed_input_commit = commit_amount_checked(
        &RistrettoSecretKey::default(),
        transfer.inputs_statement.revealed_amount,
    )
    .ok_or_else(|| ResourceError::InvalidBalanceProof {
        details: "Revealed input amount must be non-negative".to_string(),
    })?;
    let revealed_output_commit = commit_amount_checked(
        &RistrettoSecretKey::default(),
        transfer.outputs_statement.revealed_output_amount,
    )
    .ok_or_else(|| ResourceError::InvalidBalanceProof {
        details: "Revealed output amount must be non-negative".to_string(),
    })?;

    let public_excess =
        agg_inputs + revealed_input_commit.as_public_key() - &agg_outputs - revealed_output_commit.as_public_key();

    debug!(
        target: LOG_TARGET,
        "Validating transfer: revealed input amount: {}, revealed output amount: {}, public excess: {}, nonce: {}",
        transfer.inputs_statement.revealed_amount,
        transfer.outputs_statement.revealed_output_amount,
        public_excess,
        balance_proof.get_public_nonce()
    );

    let message = messages::stealth_transfer64(
        &public_excess,
        balance_proof.get_public_nonce(),
        &transfer.inputs_statement.revealed_amount,
        &transfer.outputs_statement.revealed_output_amount,
    );

    if !balance_proof.verify_raw_uniform(&public_excess, &message) {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Balance proof signature verification failed. This typically indicates that the transfer \
                      statement total input amount != total output amount."
                .to_string(),
        });
    }

    Ok(ValidatedStealthTransfer {
        outputs: validated_outputs,
        revealed_output_amount: transfer.outputs_statement.revealed_output_amount,
    })
}

fn basic_validations(transfer: &StealthTransferStatement) -> Result<(), ResourceError> {
    if transfer.inputs_statement.revealed_amount.is_negative() {
        return Err(ResourceError::InvalidBalanceProof {
            details: format!(
                "Revealed input amount must be non-negative: {}",
                transfer.inputs_statement.revealed_amount
            ),
        });
    }
    if transfer.outputs_statement.revealed_output_amount.is_negative() {
        return Err(ResourceError::InvalidBalanceProof {
            details: format!(
                "Revealed output amount must be non-negative: {}",
                transfer.outputs_statement.revealed_output_amount
            ),
        });
    }

    if transfer.inputs_statement.revealed_amount.is_zero() && transfer.inputs_statement.inputs.is_empty() {
        return Err(ResourceError::InvalidBalanceProof {
            details: "No inputs or revealed inputs provided".to_string(),
        });
    }

    if transfer.inputs_statement.inputs.is_empty() &&
        transfer.outputs_statement.outputs.is_empty() &&
        transfer.inputs_statement.revealed_amount != transfer.outputs_statement.revealed_output_amount
    {
        return Err(ResourceError::InvalidBalanceProof {
            details: format!(
                "Revealed input amount {} does not match revealed output amount {} - no stealth inputs or outputs \
                 provided",
                transfer.inputs_statement.revealed_amount, transfer.outputs_statement.revealed_output_amount
            ),
        });
    }

    Ok(())
}
