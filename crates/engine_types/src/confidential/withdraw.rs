//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{Amount, confidential::ConfidentialWithdrawProof};

use super::{ValidatedConfidentialProof, validate_confidential_statement};
use crate::{
    crypto::{ValidateOutputBody, commit_amount, messages, try_decode_to_signature},
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

/// The native-execution metering points [`validate_confidential_withdraw`] costs for this proof.
/// Charged against the payment-funded allowance *before* any verification runs, so a non-paying
/// transaction cannot extract the crypto work for free. Must stay in lock-step with what
/// [`validate_confidential_withdraw`] actually verifies: a proof that is
/// [`ConfidentialWithdrawProof::is_revealed_only`] short-circuits to a plain balance check with no
/// crypto, so it prices at zero; every other proof is charged at least the balance-proof
/// verification, plus the per-item crypto for each input and output.
pub fn withdraw_native_points(proof: &ConfidentialWithdrawProof, has_view_key: bool) -> u64 {
    use crate::limits::NativeExecutionPoints as P;
    if proof.is_revealed_only() {
        return 0;
    }
    let num_outputs = proof.output_proof.num_confidential_outputs() as u64;
    let surcharge = if has_view_key {
        P::PER_OUTPUT_VIEWABLE_SURCHARGE
    } else {
        0
    };
    let per_output = P::PER_OUTPUT.saturating_add(surcharge);
    P::PER_STATEMENT
        .saturating_add(per_output.saturating_mul(num_outputs))
        .saturating_add(P::PER_INPUT.saturating_mul(proof.inputs.len() as u64))
}

fn validate_revealed_only_withdraw(
    withdraw_proof: &ConfidentialWithdrawProof,
    total_output_revealed_amount: Amount,
) -> Result<ValidatedConfidentialWithdrawProof, ResourceError> {
    let input_revealed_amount = withdraw_proof.input_revealed_amount;
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
    Ok(ValidatedConfidentialWithdrawProof {
        output: None,
        change_output: None,
        input_revealed_amount,
        output_revealed_amount: withdraw_proof.output_proof.output_revealed_amount,
        change_revealed_amount: withdraw_proof.output_proof.change_revealed_amount,
    })
}

fn is_empty_non_revealed_withdraw(withdraw_proof: &ConfidentialWithdrawProof) -> bool {
    let stmt = &withdraw_proof.output_proof;
    withdraw_proof.inputs.is_empty() &&
        stmt.output.is_none() &&
        stmt.change_statement.is_none() &&
        stmt.range_proof.is_empty()
}

/// Returns the total revealed output amount (output + change). The balance proof signs this total, so it must fit in
/// an `Amount`.
fn check_revealed_amounts(withdraw_proof: &ConfidentialWithdrawProof) -> Result<Amount, ResourceError> {
    let output_revealed_amount = withdraw_proof.output_proof.output_revealed_amount;
    let change_revealed_amount = withdraw_proof.output_proof.change_revealed_amount;
    output_revealed_amount
        .checked_add(change_revealed_amount)
        .ok_or_else(|| ResourceError::InvalidConfidentialProof {
            details: format!(
                "Output revealed amount {output_revealed_amount} + change revealed amount {change_revealed_amount} \
                 overflows the maximum amount {}",
                Amount::MAX
            ),
        })
}

fn compute_public_excess<'a, I: IntoIterator<Item = &'a PedersenCommitment>>(
    inputs: I,
    validated_proof: &ValidatedConfidentialProof,
    input_revealed_amount: Amount,
    total_output_revealed_amount: Amount,
) -> RistrettoPublicKey {
    // Revealed amounts are public values, so they commit over the full `Amount` range.
    // 0.G + v.H - users may convert revealed funds to confidential outputs so this must be part of the balance proof
    let revealed_input_commitment = commit_amount(&Default::default(), input_revealed_amount);
    // 0.G + v.H
    let revealed_output_commitment = commit_amount(&Default::default(), total_output_revealed_amount);

    // k.G + v.H or 0.G if None
    let output_commitment = validated_proof
        .output
        .as_ref()
        .map(|o| o.commitment.as_public_key().clone())
        .unwrap_or_default();

    let agg_inputs_with_revealed = inputs.into_iter().fold(RistrettoPublicKey::default(), |sum, commit| {
        sum + commit.as_public_key()
    }) + revealed_input_commitment.as_public_key();

    let output_commitment_with_revealed = output_commitment + revealed_output_commitment.as_public_key();

    agg_inputs_with_revealed -
        &output_commitment_with_revealed -
        validated_proof
            .change_output
            .as_ref()
            .map(|output| output.commitment.as_public_key())
            .unwrap_or(&RistrettoPublicKey::default())
}

pub(crate) fn validate_confidential_withdraw<'a, I: IntoIterator<Item = &'a PedersenCommitment>>(
    inputs: I,
    view_key: Option<&RistrettoPublicKey>,
    withdraw_proof: ConfidentialWithdrawProof,
) -> Result<ValidatedConfidentialWithdrawProof, ResourceError> {
    let input_revealed_amount = withdraw_proof.input_revealed_amount;
    let total_output_revealed_amount = check_revealed_amounts(&withdraw_proof)?;

    // Balance proof not required if only revealed funds are transferred
    if withdraw_proof.is_revealed_only() {
        return validate_revealed_only_withdraw(&withdraw_proof, total_output_revealed_amount);
    }

    // With no inputs and no confidential outputs the public excess is a multiple of H alone. A non-zero multiple has
    // no known discrete log, and the signature implementation rejects the identity key, so no balance proof can verify
    // and revealed-only is the only valid form of this shape.
    if is_empty_non_revealed_withdraw(&withdraw_proof) {
        return Err(ResourceError::InvalidBalanceProof {
            details: "A withdraw with no inputs and no confidential outputs must be revealed-only: a zero balance \
                      proof, an empty range proof and non-zero revealed amounts"
                .to_string(),
        });
    }

    let balance_proof =
        try_decode_to_signature(&withdraw_proof.balance_proof).ok_or_else(|| ResourceError::InvalidBalanceProof {
            details: "Malformed balance proof".to_string(),
        })?;

    let validated_proof = validate_confidential_statement(&withdraw_proof.output_proof, view_key)?;
    let public_excess = compute_public_excess(
        inputs,
        &validated_proof,
        input_revealed_amount,
        total_output_revealed_amount,
    );

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
        input_revealed_amount,
        output_revealed_amount: withdraw_proof.output_proof.output_revealed_amount,
        change_revealed_amount: withdraw_proof.output_proof.change_revealed_amount,
    })
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{keys::PublicKey, ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
    use tari_template_lib::types::{
        EncryptedData,
        confidential::ConfidentialOutputStatement,
        constants::STEALTH_TARI_RESOURCE_ADDRESS,
        crypto::{BalanceProofSignature, RangeProofBytes, RistrettoPublicKeyBytes, Scalar32Bytes},
        stealth::UnspentOutput,
    };

    use super::*;
    use crate::{
        crypto::commit_u64_amount,
        hashing::EngineSchnorrSignature,
        limits::NativeExecutionPoints,
        resource_container::ResourceContainer,
    };

    /// A revealed withdraw carrying a decodable, non-zero balance proof: no inputs and no
    /// confidential outputs, but not revealed-only.
    fn revealed_shape_with_balance_proof() -> ConfidentialWithdrawProof {
        let nonce = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(1u64));
        let mut proof = ConfidentialWithdrawProof::revealed_withdraw(1000u64);
        proof.balance_proof = BalanceProofSignature::new(
            RistrettoPublicKeyBytes::from_bytes(nonce.as_bytes()).unwrap(),
            Scalar32Bytes::from_bytes(RistrettoSecretKey::from(1u64).as_bytes()).unwrap(),
        );
        proof
    }

    fn withdraw_with_bogus_range_proof() -> ConfidentialWithdrawProof {
        let mask = RistrettoSecretKey::from(7u64);
        let commitment = commit_u64_amount(&mask, 100).to_byte_type();
        let sender_public_nonce = RistrettoPublicKey::from_secret_key(&mask).to_byte_type();
        let mut proof = revealed_shape_with_balance_proof();
        proof.output_proof.output = Some(UnspentOutput {
            commitment,
            sender_public_nonce,
            encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
            minimum_value_promise: 1,
            viewable_balance_proof: None,
        });
        // Bogus non-empty range proof that would fail inside `validate_confidential_statement` with
        // `InvalidRangeProof`.
        proof.output_proof.range_proof = RangeProofBytes::try_from(vec![1u8; 64]).unwrap();
        proof
    }

    #[test]
    fn revealed_only_withdraw_prices_at_zero() {
        let proof = ConfidentialWithdrawProof::revealed_withdraw(1000u64);
        assert!(proof.is_revealed_only());
        assert_eq!(withdraw_native_points(&proof, false), 0);
        validate_confidential_withdraw([], None, proof).unwrap();
    }

    #[test]
    fn withdraw_without_inputs_or_outputs_that_is_not_revealed_only_is_priced() {
        let proof = revealed_shape_with_balance_proof();
        assert!(!proof.is_revealed_only());
        assert_eq!(
            withdraw_native_points(&proof, false),
            NativeExecutionPoints::PER_STATEMENT
        );
    }

    #[test]
    fn withdraw_without_inputs_or_outputs_that_is_not_revealed_only_is_rejected_before_verification() {
        let err = validate_confidential_withdraw([], None, revealed_shape_with_balance_proof()).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidBalanceProof { details } if details.contains("must be revealed-only")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn revealed_only_withdraw_returns_revealed_change_to_the_source() {
        let mut proof = ConfidentialWithdrawProof::revealed_withdraw(100u64);
        proof.output_proof.output_revealed_amount = Amount::new(40);
        proof.output_proof.change_revealed_amount = Amount::new(60);
        assert!(proof.is_revealed_only());

        let mut source = ResourceContainer::confidential(STEALTH_TARI_RESOURCE_ADDRESS, None, Amount::new(150));
        let (output, _) = source.withdraw_confidential(proof, None).unwrap();

        assert_eq!(output.unlocked_amount(), Amount::new(40));
        assert_eq!(source.unlocked_amount(), Amount::new(110));
    }

    #[test]
    fn withdraw_rejects_overflowing_revealed_amounts_and_malformed_balance_proof_before_statement_crypto() {
        let mut overflow_proof = withdraw_with_bogus_range_proof();
        overflow_proof.output_proof.output_revealed_amount = Amount::MAX;
        overflow_proof.output_proof.change_revealed_amount = Amount::from(1u64);
        let err = validate_confidential_withdraw([], None, overflow_proof).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details } if details.contains("overflows")
            ),
            "unexpected error: {err:?}"
        );

        let mut malformed_sig = withdraw_with_bogus_range_proof();
        malformed_sig.balance_proof = BalanceProofSignature::new(
            RistrettoPublicKeyBytes::from([0xff; 32]),
            Scalar32Bytes::from([0xff; 32]),
        );
        let err = validate_confidential_withdraw([], None, malformed_sig).unwrap_err();
        assert!(matches!(
            &err,
            ResourceError::InvalidBalanceProof { details } if details.contains("Malformed balance proof")
        ));
    }

    #[test]
    fn withdraw_balances_revealed_amounts_above_u64_max() {
        let input_mask = RistrettoSecretKey::from(11u64);
        let input = commit_u64_amount(&input_mask, 10);
        let input_revealed_amount = Amount::new(1u128 << 64);
        let output_revealed_amount = input_revealed_amount + 10u64;

        // k.G + 10.H + 2^64.H - (2^64 + 10).H = k.G
        let public_excess = RistrettoPublicKey::from_secret_key(&input_mask);
        let nonce_secret = RistrettoSecretKey::from(999u64);
        let nonce_public = RistrettoPublicKey::from_secret_key(&nonce_secret);
        let message = messages::confidential_withdraw64(
            &public_excess,
            &nonce_public,
            &input_revealed_amount,
            &output_revealed_amount,
        );
        let balance_proof = EngineSchnorrSignature::sign_raw_uniform(&input_mask, nonce_secret, &message)
            .unwrap()
            .to_byte_type();

        let proof = ConfidentialWithdrawProof {
            inputs: vec![input.to_byte_type()],
            input_revealed_amount,
            output_proof: ConfidentialOutputStatement::mint_revealed(output_revealed_amount),
            balance_proof,
        };
        assert!(!proof.is_revealed_only());

        let validated = validate_confidential_withdraw([&input], None, proof).unwrap();
        assert!(validated.output.is_none());
        assert!(validated.change_output.is_none());
        assert_eq!(validated.input_revealed_amount, input_revealed_amount);
        assert_eq!(validated.output_revealed_amount, output_revealed_amount);
        assert_eq!(validated.change_revealed_amount, Amount::zero());
    }
}
