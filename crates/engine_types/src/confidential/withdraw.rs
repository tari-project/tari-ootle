//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::PrivateKey;
use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::{prelude::ConfidentialWithdrawProof, types::Amount};

use super::validate_confidential_statement;
use crate::{
    crypto::{commit_amount, messages, try_decode_to_signature, ValidateOutputBody},
    resource_container::ResourceError,
};

#[derive(Debug, Clone)]
pub struct ValidatedConfidentialWithdrawProof {
    /// Optional confidential output of the withdraw. This will be created as a new output commitment.
    pub output: Option<ValidateOutputBody>,
    /// Optional confidential change output of the withdraw. This will replace any inputs used.
    pub change_output: Option<ValidateOutputBody>,
    /// Amount of revealed value to use as an input.
    pub input_revealed_amount: Amount,
    /// Amount of revealed value to include in the revealed value of the output
    pub output_revealed_amount: Amount,
    /// Amount of revealed value to include in the revealed value of the change output
    pub change_revealed_amount: Amount,
}

pub(crate) fn validate_confidential_withdraw<'a, I: IntoIterator<Item = &'a PedersenCommitment>>(
    inputs: I,
    view_key: Option<&RistrettoPublicKey>,
    withdraw_proof: ConfidentialWithdrawProof,
) -> Result<ValidatedConfidentialWithdrawProof, ResourceError> {
    let validated_proof = validate_confidential_statement(&withdraw_proof.output_proof, view_key)?;

    let input_revealed_amount = withdraw_proof.input_revealed_amount;
    if input_revealed_amount.is_negative() {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Input revealed amount cannot be negative".to_string(),
        });
    }
    // We expect the revealed amount to be excluded from the output commitment.
    let total_output_revealed_amount = withdraw_proof
        .output_proof
        .output_revealed_amount
        .checked_add(withdraw_proof.output_proof.change_revealed_amount)
        .ok_or_else(|| ResourceError::InvalidConfidentialProof {
            details: format!(
                "Output revealed amount {} + change revealed amount {} cannot be negative",
                withdraw_proof.output_proof.output_revealed_amount, withdraw_proof.output_proof.change_revealed_amount
            ),
        })?;

    // Balance proof not required if only revealed funds are transferred
    if withdraw_proof.is_revealed_only() {
        if input_revealed_amount
            .checked_sub(total_output_revealed_amount)
            .is_none_or(|v| !v.is_zero())
        {
            return Err(ResourceError::InvalidBalanceProof {
                details: "Incorrect balance for revealed only withdraw proof".to_string(),
            });
        }

        // This only contains revealed funds transfer, so a simple balance check is all that's needed.
        // The given zero signature _would_ be valid (R + e.0.G == (r + e.0).G), however the signature implementation
        // correctly disallows the zero key. See [ConfidentialWithdrawProof::revealed_withdraw].
        return Ok(ValidatedConfidentialWithdrawProof {
            output: None,
            change_output: validated_proof.change_output,
            input_revealed_amount,
            output_revealed_amount: withdraw_proof.output_proof.output_revealed_amount,
            change_revealed_amount: withdraw_proof.output_proof.change_revealed_amount,
        });
    }

    let balance_proof =
        try_decode_to_signature(&withdraw_proof.balance_proof).ok_or_else(|| ResourceError::InvalidBalanceProof {
            details: "Malformed balance proof".to_string(),
        })?;

    // k.G + v.H or 0.G if None
    let output_commitment = validated_proof
        .output
        .as_ref()
        .map(|o| o.commitment.as_public_key().clone())
        .unwrap_or_default();

    // 0.G + v.H - users may convert revealed funds to confidential outputs so this must be part of the balance proof
    // PANIC: We already checked that input_revealed_amount is non-negative
    let revealed_input_commitment = commit_amount(&PrivateKey::default(), input_revealed_amount);
    let agg_inputs_with_revealed = inputs.into_iter().fold(RistrettoPublicKey::default(), |sum, commit| {
        sum + commit.as_public_key()
    }) + revealed_input_commitment.as_public_key();

    // 0.G + v.H
    // PANIC: We already checked that total_output_revealed_amount is positive
    let revealed_output_commitment = commit_amount(&PrivateKey::default(), total_output_revealed_amount);
    let output_commitment_with_revealed = output_commitment + revealed_output_commitment.as_public_key();

    let public_excess = agg_inputs_with_revealed -
        &output_commitment_with_revealed -
        validated_proof
            .change_output
            .as_ref()
            .map(|output| output.commitment.as_public_key())
            .unwrap_or(&RistrettoPublicKey::default());

    let message = messages::confidential_withdraw64(
        &public_excess,
        balance_proof.get_public_nonce(),
        &input_revealed_amount,
        &total_output_revealed_amount,
    );

    if !balance_proof.verify_raw_uniform(&public_excess, &message) {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Balance proof was invalid".to_string(),
        });
    }

    Ok(ValidatedConfidentialWithdrawProof {
        output: validated_proof.output,
        change_output: validated_proof.change_output,
        input_revealed_amount: withdraw_proof.input_revealed_amount,
        output_revealed_amount: withdraw_proof.output_proof.output_revealed_amount,
        change_revealed_amount: withdraw_proof.output_proof.change_revealed_amount,
    })
}
