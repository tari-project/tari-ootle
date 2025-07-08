//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_common_types::types::{BulletRangeProof, PrivateKey};
use tari_crypto::{
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSchnorr},
    tari_utilities::ByteArray,
};
use tari_template_lib::{
    models::{ConfidentialWithdrawProof, EncryptedData},
    types::{
        crypto::{BalanceProofSignature, RistrettoPublicKeyBytes},
        Amount,
    },
};

use super::{commit_amount, messages, validate_confidential_proof, CompressedElgamalVerifiableBalance};
use crate::{
    confidential::elgamal::ElgamalVerifiableBalance,
    resource_container::ResourceError,
    FromByteType,
    ToByteType,
};

#[derive(Debug, Clone)]
pub struct ValidatedConfidentialWithdrawProof {
    /// Optional confidential output of the withdraw. This will be created as a new output commitment.
    pub output: Option<ValidatedConfidentialOutput>,
    /// Optional confidential change output of the withdraw. This will replace any inputs used.
    pub change_output: Option<ValidatedConfidentialOutput>,
    /// Range proof
    pub range_proof: BulletRangeProof,
    /// Amount of revealed value to use as an input.
    pub input_revealed_amount: Amount,
    /// Amount of revealed value to include in the revealed value of the output
    pub output_revealed_amount: Amount,
    /// Amount of revealed value to include in the revealed value of the change output
    pub change_revealed_amount: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct ConfidentialOutput {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub stealth_public_nonce: RistrettoPublicKeyBytes,
    #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
    pub encrypted_data: EncryptedData,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<CompressedElgamalVerifiableBalance>,
}

impl From<ValidatedConfidentialOutput> for ConfidentialOutput {
    fn from(output: ValidatedConfidentialOutput) -> Self {
        Self {
            stealth_public_nonce: output.stealth_public_nonce.to_byte_type(),
            encrypted_data: output.encrypted_data,
            minimum_value_promise: output.minimum_value_promise,
            viewable_balance: output.viewable_balance.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedConfidentialOutput {
    pub commitment: PedersenCommitment,
    pub stealth_public_nonce: RistrettoPublicKey,
    pub encrypted_data: EncryptedData,
    pub minimum_value_promise: u64,
    pub viewable_balance: Option<ElgamalVerifiableBalance>,
}

pub(crate) fn validate_confidential_withdraw<'a, I: IntoIterator<Item = &'a PedersenCommitment>>(
    inputs: I,
    view_key: Option<&RistrettoPublicKey>,
    withdraw_proof: ConfidentialWithdrawProof,
) -> Result<ValidatedConfidentialWithdrawProof, ResourceError> {
    let validated_proof = validate_confidential_proof(&withdraw_proof.output_proof, view_key)?;

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
        .checked_add_positive(withdraw_proof.output_proof.change_revealed_amount)
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
        // The given zero signature _would_ be valid (public_excess == (0)), however the signature implementation
        // correctly disallows the zero key. See [ConfidentialWithdrawProof::revealed_withdraw].
        return Ok(ValidatedConfidentialWithdrawProof {
            output: None,
            change_output: validated_proof.change_output,
            range_proof: BulletRangeProof(withdraw_proof.output_proof.range_proof),
            input_revealed_amount,
            output_revealed_amount: withdraw_proof.output_proof.output_revealed_amount,
            change_revealed_amount: withdraw_proof.output_proof.change_revealed_amount,
        });
    }

    // k.G + v.H or 0.G if None
    let output_commitment = validated_proof
        .output
        .as_ref()
        .map(|o| o.commitment.as_public_key().clone())
        .unwrap_or_default();

    // 0.G + v.H
    // We already checked that total_output_revealed_amount is positive
    let revealed_output_commitment = commit_amount(&PrivateKey::default(), total_output_revealed_amount);
    let output_commitment_with_revealed = output_commitment + revealed_output_commitment.as_public_key();

    let balance_proof =
        try_decode_to_signature(&withdraw_proof.balance_proof).ok_or_else(|| ResourceError::InvalidBalanceProof {
            details: "Malformed balance proof".to_string(),
        })?;

    // 0.G + v.H - users may convert revealed funds to confidential outputs so this must be part of the balance proof
    // We already checked that input_revealed_amount is non-negative
    let revealed_input_commitment = commit_amount(&PrivateKey::default(), input_revealed_amount);
    let agg_inputs = inputs.into_iter().fold(RistrettoPublicKey::default(), |sum, commit| {
        sum + commit.as_public_key()
    }) + revealed_input_commitment.as_public_key();

    let public_excess = agg_inputs -
        &output_commitment_with_revealed -
        validated_proof
            .change_output
            .as_ref()
            .map(|output| output.commitment.as_public_key())
            .unwrap_or(&RistrettoPublicKey::default());

    let message = messages::confidential_withdraw64(
        &public_excess,
        balance_proof.get_public_nonce(),
        input_revealed_amount,
        total_output_revealed_amount,
    );

    if !balance_proof.verify_raw_uniform(&public_excess, &message) {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Balance proof was invalid".to_string(),
        });
    }

    Ok(ValidatedConfidentialWithdrawProof {
        output: validated_proof.output,
        change_output: validated_proof.change_output,
        range_proof: BulletRangeProof(withdraw_proof.output_proof.range_proof),
        input_revealed_amount: withdraw_proof.input_revealed_amount,
        output_revealed_amount: withdraw_proof.output_proof.output_revealed_amount,
        change_revealed_amount: withdraw_proof.output_proof.change_revealed_amount,
    })
}

fn try_decode_to_signature(balance_proof: &BalanceProofSignature) -> Option<RistrettoSchnorr> {
    let public_nonce = RistrettoPublicKey::try_from_byte_type(balance_proof.public_nonce()).ok()?;
    let signature = PrivateKey::from_canonical_bytes(balance_proof.signature().as_bytes()).ok()?;
    Some(RistrettoSchnorr::new(public_nonce, signature))
}
