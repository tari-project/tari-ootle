//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoPublicKey;
use tari_template_lib::types::{Amount, confidential::ConfidentialOutputStatement};

use crate::{
    crypto::{DecodedOutput, ValidateOutputBody, range_proof::validate_bullet_proof_statements},
    resource_container::ResourceError,
};

#[derive(Debug)]
pub struct ValidatedConfidentialProof {
    pub output: Option<ValidateOutputBody>,
    pub change_output: Option<ValidateOutputBody>,
    pub output_revealed_amount: Amount,
    pub change_revealed_amount: Amount,
}

/// The native-execution metering points [`validate_confidential_statement`] costs for this
/// statement (range proof per present output, plus the fixed base, plus the viewable-balance
/// proof per output when the resource carries a view key). Charged against the payment-funded
/// allowance *before* verification runs. Must stay in lock-step with what
/// [`validate_confidential_statement`] actually verifies: a statement with no confidential
/// outputs (revealed-only) verifies no range proof, so it prices at zero.
pub fn statement_native_points(proof: &ConfidentialOutputStatement, has_view_key: bool) -> u64 {
    use crate::limits::NativeExecutionPoints as P;
    let num_outputs = proof.num_confidential_outputs() as u64;
    if num_outputs == 0 {
        return 0;
    }
    let per_output = P::PER_OUTPUT.saturating_add(if has_view_key {
        P::PER_OUTPUT_VIEWABLE_SURCHARGE
    } else {
        0
    });
    P::PER_STATEMENT.saturating_add(per_output.saturating_mul(num_outputs))
}

/// The least `minimum_value_promise` a confidential output may carry. A vault's commitments count as holding the
/// resource (see [`crate::resource_container::ResourceContainer::is_empty`]), which is sound only while the range proof
/// binds every commitment to a value of at least this.
pub const MIN_CONFIDENTIAL_OUTPUT_VALUE_PROMISE: u64 = 1;

pub fn validate_confidential_statement(
    proof: &ConfidentialOutputStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedConfidentialProof, ResourceError> {
    if proof
        .output
        .iter()
        .chain(&proof.change_statement)
        .any(|o| o.minimum_value_promise < MIN_CONFIDENTIAL_OUTPUT_VALUE_PROMISE)
    {
        return Err(ResourceError::InvalidConfidentialProof {
            details: format!(
                "A confidential output must promise a minimum value of at least \
                 {MIN_CONFIDENTIAL_OUTPUT_VALUE_PROMISE}"
            ),
        });
    }

    if let (Some(output), Some(change)) = (&proof.output, &proof.change_statement) &&
        output.commitment == change.commitment
    {
        return Err(ResourceError::InvalidConfidentialProof {
            details: "Confidential output and change commitments cannot be identical".to_string(),
        });
    }

    let decoded_output = proof
        .output
        .as_ref()
        .map(|output| DecodedOutput::decode(output, view_key))
        .transpose()?;
    let decoded_change = proof
        .change_statement
        .as_ref()
        .map(|change| DecodedOutput::decode(change, view_key))
        .transpose()?;

    // The aggregated range proof covers the output then the change, in that order.
    let bp_statements = decoded_output
        .iter()
        .chain(&decoded_change)
        .map(DecodedOutput::to_range_proof_statement)
        .collect::<Vec<_>>();
    if bp_statements.is_empty() {
        if !proof.range_proof.is_empty() {
            return Err(ResourceError::InvalidConfidentialProof {
                details: "Range proof is invalid because it was provided (non-empty) but the proof contained no \
                          confidential outputs"
                    .to_string(),
            });
        }
        return Ok(ValidatedConfidentialProof {
            output: None,
            change_output: None,
            output_revealed_amount: proof.output_revealed_amount,
            change_revealed_amount: proof.change_revealed_amount,
        });
    }

    if proof.range_proof.is_empty() {
        return Err(ResourceError::InvalidRangeProof {
            details: "Range proof is empty".to_string(),
        });
    }

    let maybe_output = decoded_output
        .map(|output| output.verify_viewable_balance(view_key))
        .transpose()?;
    let maybe_change = decoded_change
        .map(|change| change.verify_viewable_balance(view_key))
        .transpose()?;

    validate_bullet_proof_statements(&proof.range_proof, bp_statements)?;

    Ok(ValidatedConfidentialProof {
        output: maybe_output,
        change_output: maybe_change,
        output_revealed_amount: proof.output_revealed_amount,
        change_revealed_amount: proof.change_revealed_amount,
    })
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{keys::PublicKey, ristretto::RistrettoSecretKey};
    use tari_template_lib::types::{EncryptedData, crypto::RangeProofBytes, stealth::UnspentOutput};

    use super::*;
    use crate::crypto::commit_u64_amount;

    fn sample_unspent_output(mask_seed: u64, value: u64) -> UnspentOutput {
        let mask = RistrettoSecretKey::from(mask_seed);
        let commitment = commit_u64_amount(&mask, value).to_byte_type();
        let sender_public_nonce = RistrettoPublicKey::from_secret_key(&mask).to_byte_type();
        UnspentOutput {
            commitment,
            sender_public_nonce,
            encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
            minimum_value_promise: 1,
            viewable_balance_proof: None,
        }
    }

    #[test]
    fn rejects_identical_output_and_change_commitments() {
        let dup_output = sample_unspent_output(7, 100);
        let stmt = ConfidentialOutputStatement {
            output: Some(dup_output.clone()),
            change_statement: Some(dup_output),
            range_proof: RangeProofBytes::empty(),
            output_revealed_amount: Amount::ZERO,
            change_revealed_amount: Amount::ZERO,
        };

        let err = validate_confidential_statement(&stmt, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("output and change commitments cannot be identical")
            ),
            "unexpected error: {err:?}"
        );
    }
}
