//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArrayError,
};
use tari_template_lib::{models::StealthTransferStatement, types::Amount};

use crate::{
    crypto::{commit_amount_checked, messages, try_decode_to_signature},
    resource_container::ResourceError,
    stealth,
    stealth::ValidatedStealthOutput,
    FromByteType,
};

#[derive(Debug, Clone)]
pub struct ValidatedStealthTransfer {
    pub outputs: Vec<ValidatedStealthOutput>,
    pub revealed_input_amount: Amount,
    pub revealed_output_amount: Amount,
}

pub fn validate_transfer(
    transfer: &StealthTransferStatement,
    revealed_input_amount: Amount,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthTransfer, ResourceError> {
    let validated_outputs = stealth::validate_stealth_outputs_statement(&transfer.outputs_statement, view_key)?;

    let balance_proof =
        try_decode_to_signature(&transfer.balance_proof).ok_or_else(|| ResourceError::InvalidBalanceProof {
            details: "Malformed balance proof".to_string(),
        })?;

    let agg_outputs = validated_outputs.iter().fold(RistrettoPublicKey::default(), |acc, o| {
        acc + o.output.commitment.as_public_key()
    });

    let agg_inputs = transfer
        .inputs
        .iter()
        .try_fold(RistrettoPublicKey::default(), |sum, input| {
            let commit = PedersenCommitment::try_from_byte_type(&input.commitment)?;
            Ok::<_, ByteArrayError>(sum + commit.as_public_key())
        })
        .map_err(|e| ResourceError::InvalidConfidentialProof {
            details: format!("Malformed commitment in transfer inputs: {e}"),
        })?;

    // We assume that the input amount is available and only check that the maths is correct. The engine is responsible
    // for checking that the input amount is actually available.
    let revealed_input_commit = commit_amount_checked(&RistrettoSecretKey::default(), revealed_input_amount)
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

    let message = messages::stealth_transfer64(
        &public_excess,
        balance_proof.get_public_nonce(),
        &revealed_input_amount,
        &transfer.outputs_statement.revealed_output_amount,
    );

    if !balance_proof.verify_raw_uniform(&public_excess, &message) {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Balance proof signature verification failed".to_string(),
        });
    }

    Ok(ValidatedStealthTransfer {
        outputs: validated_outputs,
        revealed_input_amount,
        revealed_output_amount: transfer.outputs_statement.revealed_output_amount,
    })
}
