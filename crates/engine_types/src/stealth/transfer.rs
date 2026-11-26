//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeSet;

use log::*;
use ootle_byte_type::ConvertFromByteType;
use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSecretKey, pedersen::PedersenCommitment},
    tari_utilities::ByteArrayError,
};
use tari_template_lib::types::stealth::{RevealedOutput, StealthTransferStatement};

use crate::{
    crypto::{commit_amount, messages, try_decode_to_signature},
    resource_container::ResourceError,
    stealth,
    stealth::ValidatedStealthOutput,
};

const LOG_TARGET: &str = "tari::ootle::engine::stealth::transfer";

#[derive(Debug, Clone)]
pub struct ValidatedStealthTransfer {
    pub outputs: Vec<ValidatedStealthOutput>,
    pub revealed_output: Option<RevealedOutput>,
}

/// The native-execution metering points [`validate_transfer`] (plus the per-input spend
/// authorisation pass) costs for this statement. Charged against the payment-funded allowance
/// *before* any verification runs, so a non-paying transaction cannot extract the crypto work for
/// free. Must stay in lock-step with what [`validate_transfer`] actually verifies: a revealed-only
/// statement (no stealth inputs or outputs) verifies no balance proof and no range proof, so it
/// prices at zero, and the per-output ElGamal viewable-balance proof is only verified (and only
/// priced) when the resource carries a view key.
pub fn transfer_native_points(transfer: &StealthTransferStatement, has_view_key: bool) -> u64 {
    transfer_native_points_for_shape(
        transfer.inputs_statement.inputs.len(),
        transfer.outputs_statement.outputs.len(),
        has_view_key,
    )
}

/// [`transfer_native_points`] priced from a statement's shape alone, before the statement is built.
/// A builder must know the cost while choosing that shape — the range proofs it would have to
/// generate to produce a statement are the very work being priced — so the cost cannot depend on
/// anything but the input and output counts and the resource's view key.
pub fn transfer_native_points_for_shape(num_inputs: usize, num_outputs: usize, has_view_key: bool) -> u64 {
    use crate::limits::NativeExecutionPoints as P;
    if num_inputs == 0 && num_outputs == 0 {
        return 0;
    }
    let per_output = P::PER_OUTPUT.saturating_add(if has_view_key {
        P::PER_OUTPUT_VIEWABLE_SURCHARGE
    } else {
        0
    });
    P::PER_STATEMENT
        .saturating_add(per_output.saturating_mul(num_outputs as u64))
        .saturating_add(P::PER_INPUT.saturating_mul(num_inputs as u64))
}

/// A [`StealthTransferStatement`] that has passed the structural checks: every commitment is unique across inputs and
/// outputs ([`validate_transfer_commitments`]) and every covenant claim names a distinct input
/// ([`validate_covenant_claims`]). [`CheckedStealthTransfer::check`] is the only way to build one, and the statement is
/// reachable by shared reference only, so holding one proves those checks ran on exactly this statement.
#[derive(Debug)]
pub struct CheckedStealthTransfer(StealthTransferStatement);

impl CheckedStealthTransfer {
    pub fn check(statement: StealthTransferStatement) -> Result<Self, ResourceError> {
        CheckedStatementRef::check(&statement)?;
        Ok(Self(statement))
    }

    pub fn statement(&self) -> &StealthTransferStatement {
        &self.0
    }

    fn as_checked_ref(&self) -> CheckedStatementRef<'_> {
        CheckedStatementRef(&self.0)
    }
}

/// The borrowed form of [`CheckedStealthTransfer`]: a statement reference that has passed the same structural checks.
/// [`CheckedStatementRef::check`] and [`CheckedStealthTransfer::as_checked_ref`] are the only ways to build one.
#[derive(Clone, Copy)]
struct CheckedStatementRef<'a>(&'a StealthTransferStatement);

impl<'a> CheckedStatementRef<'a> {
    /// Covenant predicates look claims up by partition, so the claim list is bounded here, before any predicate runs.
    fn check(statement: &'a StealthTransferStatement) -> Result<Self, ResourceError> {
        validate_transfer_commitments(statement)?;
        validate_covenant_claims(statement)?;
        Ok(Self(statement))
    }

    fn statement(self) -> &'a StealthTransferStatement {
        self.0
    }
}

/// Validates `transfer` in full: the structural checks of [`CheckedStealthTransfer::check`], then the revealed
/// amounts, balance proof and range proof.
pub fn validate_transfer(
    transfer: &StealthTransferStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthTransfer, ResourceError> {
    validate_checked_statement(CheckedStatementRef::check(transfer)?, view_key)
}

/// Validates the revealed amounts, balance proof and range proof of a transfer that holds the structural checks of
/// [`CheckedStealthTransfer`].
pub fn validate_checked_transfer(
    transfer: &CheckedStealthTransfer,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthTransfer, ResourceError> {
    validate_checked_statement(transfer.as_checked_ref(), view_key)
}

/// Validates the revealed amounts, balance proof and range proof of `transfer`. The witness holds unique commitments
/// across inputs and outputs, which [`stealth::validate_outputs`] requires.
fn validate_checked_statement(
    transfer: CheckedStatementRef<'_>,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<ValidatedStealthTransfer, ResourceError> {
    let transfer = transfer.statement();
    check_revealed_amounts_and_balance_proof_presence(transfer)?;
    let validated_outputs = stealth::validate_outputs(&transfer.outputs_statement, view_key)?;

    let balance_proof = transfer
        .balance_proof
        .as_ref()
        .map(|s| {
            try_decode_to_signature(s).ok_or_else(|| ResourceError::InvalidBalanceProof {
                details: "Malformed balance proof".to_string(),
            })
        })
        .transpose()?;

    // EDGE CASE: If there are no inputs and no outputs, the public excess will be 0.G
    if transfer.inputs_statement.inputs.is_empty() && validated_outputs.is_empty() {
        // Ensure that the range proof is empty
        if !transfer.outputs_statement.agg_range_proof.is_empty() {
            return Err(ResourceError::InvalidBalanceProof {
                details: "Range proof must be empty when there are no inputs or outputs".to_string(),
            });
        }

        // In this case, the public excess is 0 (s = r + e.0 = r) which leaks the secret nonce. Probably fine, but
        // instead, we enforce that a None signature MUST be used for this case.
        if balance_proof.is_some() {
            return Err(ResourceError::InvalidBalanceProof {
                details: "Balance proof signature verification failed for revealed amount. This indicates that the \
                          transfer statement provided a balance proof when there are no stealth inputs or outputs, \
                          None is required."
                    .to_string(),
            });
        }

        return Ok(ValidatedStealthTransfer {
            outputs: vec![],
            revealed_output: transfer.outputs_statement.revealed_output,
        });
    }
    let balance_proof = balance_proof.ok_or_else(|| ResourceError::InvalidBalanceProof {
        details: "Balance proof must be provided when there are stealth inputs or outputs".to_string(),
    })?;

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
    // Revealed amounts are public and carry no range proof, so they are committed at full `Amount` width.
    let revealed_input_commit = commit_amount(
        &RistrettoSecretKey::default(),
        transfer.inputs_statement.revealed_amount,
    );
    let revealed_output_commit = commit_amount(
        &RistrettoSecretKey::default(),
        transfer.outputs_statement.revealed_output_amount(),
    );

    let public_excess =
        agg_inputs + revealed_input_commit.as_public_key() - &agg_outputs - revealed_output_commit.as_public_key();

    debug!(
        target: LOG_TARGET,
        "Validating transfer: revealed input amount: {}, revealed output amount: {}, public excess: {}, nonce: {}",
        transfer.inputs_statement.revealed_amount,
        transfer.outputs_statement.revealed_output_amount(),
        public_excess,
        balance_proof.get_public_nonce()
    );

    let message = messages::stealth_balance_proof64(
        &public_excess,
        balance_proof.get_public_nonce(),
        &transfer.inputs_statement,
        &transfer.outputs_statement,
        &transfer.covenant_claims,
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
        revealed_output: transfer.outputs_statement.revealed_output,
    })
}

/// Checks that each covenant balance claim names a distinct input of `transfer`. A claim names its partition by the
/// index of the partition's first input, so a valid statement carries at most one claim per input.
pub fn validate_covenant_claims(transfer: &StealthTransferStatement) -> Result<(), ResourceError> {
    let mut claimed = BTreeSet::new();
    for claim in &transfer.covenant_claims {
        let index = claim.partition_input_index;
        let num_inputs = transfer.inputs_statement.inputs.len();
        if index as usize >= num_inputs {
            return Err(ResourceError::InvalidSpend {
                details: format!(
                    "Covenant balance claim names input {index}, but the transfer has {num_inputs} inputs"
                ),
            });
        }
        if !claimed.insert(index) {
            return Err(ResourceError::InvalidSpend {
                details: format!("Transfer carries a second claim for input {index}"),
            });
        }
    }
    Ok(())
}

/// Checks that every stealth commitment in `transfer` is unique across both inputs and outputs:
/// - no input commitment appears more than once,
/// - no output commitment duplicates an input commitment, and
/// - no output commitment appears more than once.
///
/// Balance proofs fold inputs and outputs positionally while the engine downs and creates each UTXO substate once.
/// [`CheckedStealthTransfer::check`] runs this, so the engine rejects a duplicate-commitment statement on admission,
/// before native execution charging, resource auth hooks, or spend-script predicates run.
pub fn validate_transfer_commitments(transfer: &StealthTransferStatement) -> Result<(), ResourceError> {
    let mut seen_inputs = BTreeSet::new();
    for input in &transfer.inputs_statement.inputs {
        if !seen_inputs.insert(input.commitment) {
            return Err(ResourceError::InvalidSpend {
                details: format!("Duplicate input commitment {} in transfer statement", input.commitment),
            });
        }
    }

    let mut seen_outputs = BTreeSet::new();
    for output in &transfer.outputs_statement.outputs {
        let commitment = output.output.commitment;
        if seen_inputs.contains(&commitment) {
            return Err(ResourceError::InvalidSpend {
                details: format!("Output commitment {commitment} duplicates an input commitment in transfer statement"),
            });
        }
        if !seen_outputs.insert(commitment) {
            return Err(ResourceError::InvalidConfidentialProof {
                details: format!("Duplicate output commitment {commitment} in stealth outputs statement"),
            });
        }
    }

    Ok(())
}

fn check_revealed_amounts_and_balance_proof_presence(transfer: &StealthTransferStatement) -> Result<(), ResourceError> {
    if let Some(revealed) = transfer.outputs_statement.revealed_output {
        // The presence of this field and the presence of a revealed bucket are the same fact, which holds only while
        // `None` is the sole encoding of "no revealed output".
        if !revealed.amount.is_positive() {
            return Err(ResourceError::InvalidBalanceProof {
                details: format!("Revealed output amount must be positive: {}", revealed.amount),
            });
        }
        // The receiver reaches the auth scope as a badge, which is compared bytewise and never decompresses it, so
        // canonicality is established here: a badge is satisfiable only by a signer whose key encodes to these bytes.
        if RistrettoPublicKey::convert_from_byte_type(&revealed.receiver).is_err() {
            return Err(ResourceError::InvalidSpend {
                details: format!(
                    "Revealed output receiver is not a canonical public key: {}",
                    revealed.receiver
                ),
            });
        }
    }

    if transfer.inputs_statement.revealed_amount.is_zero() && transfer.inputs_statement.inputs.is_empty() {
        return Err(ResourceError::InvalidBalanceProof {
            details: "No inputs or revealed inputs provided".to_string(),
        });
    }

    // Check the balance if there are no stealth inputs or outputs. Since the excess will be zero in this case, the
    // balance signature (r + 0.e) does not prove the balance.
    if transfer.inputs_statement.inputs.is_empty() && transfer.outputs_statement.outputs.is_empty() {
        if transfer.inputs_statement.revealed_amount != transfer.outputs_statement.revealed_output_amount() {
            return Err(ResourceError::InvalidBalanceProof {
                details: format!(
                    "Revealed input amount {} does not match revealed output amount {} - no stealth inputs or outputs \
                     provided",
                    transfer.inputs_statement.revealed_amount,
                    transfer.outputs_statement.revealed_output_amount()
                ),
            });
        }
    } else if transfer
        .balance_proof
        .is_none_or(|p| p.public_nonce().is_zero() || p.signature().is_zero())
    {
        return Err(ResourceError::InvalidBalanceProof {
            details: "Balance proof must be provided and public nonce and signature cannot be zero".to_string(),
        });
    } else {
        // Ok
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::NativeExecutionPoints as P;

    #[test]
    fn empty_statement_prices_at_zero() {
        // A revealed-only statement verifies no balance proof and no range proof, matching the
        // short-circuit in `validate_transfer`.
        assert_eq!(transfer_native_points_for_shape(0, 0, false), 0);
        assert_eq!(transfer_native_points_for_shape(0, 0, true), 0);
    }

    #[test]
    fn price_is_statement_plus_per_output_plus_per_input() {
        assert_eq!(
            transfer_native_points_for_shape(3, 2, false),
            P::PER_STATEMENT + 2 * P::PER_OUTPUT + 3 * P::PER_INPUT
        );
    }

    #[test]
    fn view_key_adds_the_surcharge_per_output() {
        let no_vk = transfer_native_points_for_shape(1, 2, false);
        let with_vk = transfer_native_points_for_shape(1, 2, true);
        assert_eq!(with_vk - no_vk, 2 * P::PER_OUTPUT_VIEWABLE_SURCHARGE);
    }

    fn make_output(seed: u64, value: u64) -> tari_template_lib::types::stealth::StealthUnspentOutput {
        use ootle_byte_type::ToByteType;
        use tari_crypto::keys::PublicKey;
        use tari_template_lib::types::{
            EncryptedData,
            crypto::UtxoTag,
            stealth::{SpendAuthorization, StealthUnspentOutput, UnspentOutput},
        };

        use crate::crypto::commit_u64_amount;

        let mask = RistrettoSecretKey::from(seed);
        let commitment = commit_u64_amount(&mask, value).to_byte_type();
        let pk = RistrettoPublicKey::from_secret_key(&mask).to_byte_type();
        StealthUnspentOutput {
            output: UnspentOutput {
                commitment,
                sender_public_nonce: pk,
                encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
                minimum_value_promise: 1,
                viewable_balance_proof: None,
            },
            auth: SpendAuthorization::Key(pk),
            tag: UtxoTag::new(0),
        }
    }

    #[test]
    fn validate_transfer_rejects_output_duplicating_input_or_sibling_output_commitment() {
        use tari_template_lib::types::{
            Amount,
            crypto::{BalanceProofSignature, RangeProofBytes, RistrettoPublicKeyBytes, Scalar32Bytes},
            stealth::{StealthInput, StealthInputsStatement, StealthOutputsStatement},
        };

        let out1 = make_output(1, 100);
        let out2 = make_output(2, 100);
        let dummy_sig =
            BalanceProofSignature::new(RistrettoPublicKeyBytes::from([1u8; 32]), Scalar32Bytes::from([1u8; 32]));

        // 1. Output commitment duplicating an input commitment is rejected with InvalidSpend.
        let input_output_collision = StealthTransferStatement {
            inputs_statement: StealthInputsStatement::new(
                vec![StealthInput::new(out1.output.commitment)],
                Amount::ZERO,
            ),
            outputs_statement: StealthOutputsStatement {
                outputs: vec![out1.clone()],
                revealed_output: None,
                agg_range_proof: RangeProofBytes::empty(),
            },
            balance_proof: Some(dummy_sig),
            covenant_claims: vec![],
        };
        let err = validate_transfer(&input_output_collision, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidSpend { details }
                    if details.contains("duplicates an input commitment")
            ),
            "unexpected error: {err:?}"
        );

        // 2. Duplicate output commitments in the same transfer statement are rejected.
        let dup_outputs = StealthTransferStatement {
            inputs_statement: StealthInputsStatement::new(
                vec![StealthInput::new(out1.output.commitment)],
                Amount::ZERO,
            ),
            outputs_statement: StealthOutputsStatement {
                outputs: vec![out2.clone(), out2],
                revealed_output: None,
                agg_range_proof: RangeProofBytes::try_from(vec![0xabu8; 64]).unwrap(),
            },
            balance_proof: Some(dummy_sig),
            covenant_claims: vec![],
        };
        let err = validate_transfer(&dup_outputs, None).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidConfidentialProof { details }
                    if details.contains("Duplicate output commitment")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_transfer_commitments_rejects_duplicates_and_accepts_unique() {
        use tari_template_lib::types::{
            Amount,
            crypto::RangeProofBytes,
            stealth::{StealthInput, StealthInputsStatement, StealthOutputsStatement},
        };

        let out1 = make_output(1, 100);
        let dup_inputs = StealthTransferStatement {
            inputs_statement: StealthInputsStatement::new(
                vec![
                    StealthInput::new(out1.output.commitment),
                    StealthInput::new(out1.output.commitment),
                ],
                Amount::ZERO,
            ),
            outputs_statement: StealthOutputsStatement {
                outputs: vec![],
                revealed_output: None,
                agg_range_proof: RangeProofBytes::empty(),
            },
            balance_proof: None,
            covenant_claims: vec![],
        };
        let err = validate_transfer_commitments(&dup_inputs).unwrap_err();
        assert!(
            matches!(
                &err,
                ResourceError::InvalidSpend { details }
                    if details.contains("Duplicate input commitment")
            ),
            "unexpected error: {err:?}"
        );

        let unique_commitments = StealthTransferStatement {
            inputs_statement: StealthInputsStatement::new(
                vec![StealthInput::new(out1.output.commitment)],
                Amount::ZERO,
            ),
            outputs_statement: StealthOutputsStatement {
                outputs: vec![make_output(3, 100)],
                revealed_output: None,
                agg_range_proof: RangeProofBytes::empty(),
            },
            balance_proof: None,
            covenant_claims: vec![],
        };
        validate_transfer_commitments(&unique_commitments).unwrap();
    }

    #[test]
    fn checked_stealth_transfer_rejects_what_validate_transfer_rejects_structurally() {
        use tari_template_lib::types::{
            Amount,
            crypto::{BalanceProofSignature, RangeProofBytes},
            stealth::{
                CovenantBalanceClaim,
                StealthInput,
                StealthInputsStatement,
                StealthOutputsStatement,
                StealthUnspentOutput,
            },
        };

        fn statement(
            inputs: Vec<StealthInput>,
            outputs: Vec<StealthUnspentOutput>,
            claim_indices: &[u32],
        ) -> StealthTransferStatement {
            StealthTransferStatement {
                inputs_statement: StealthInputsStatement::new(inputs, Amount::ZERO),
                outputs_statement: StealthOutputsStatement {
                    outputs,
                    revealed_output: None,
                    agg_range_proof: RangeProofBytes::empty(),
                },
                balance_proof: None,
                covenant_claims: claim_indices
                    .iter()
                    .map(|&partition_input_index| CovenantBalanceClaim {
                        partition_input_index,
                        revealed_amount: Amount::ZERO,
                        signature: BalanceProofSignature::zero(),
                    })
                    .collect(),
            }
        }

        let out1 = make_output(1, 100);
        let out2 = make_output(2, 100);
        let input1 = StealthInput::new(out1.output.commitment);

        let cases = [
            (
                statement(vec![input1.clone(), input1.clone()], vec![], &[]),
                "Duplicate input commitment",
            ),
            (
                statement(vec![input1.clone()], vec![out1.clone()], &[]),
                "duplicates an input commitment",
            ),
            (
                statement(vec![input1.clone()], vec![out2.clone(), out2.clone()], &[]),
                "Duplicate output commitment",
            ),
            (
                statement(vec![input1.clone()], vec![out2.clone()], &[1]),
                "names input 1, but the transfer has 1 inputs",
            ),
            (
                statement(vec![input1.clone()], vec![out2.clone()], &[0, 0]),
                "a second claim for input 0",
            ),
        ];
        for (stmt, expected) in cases {
            let unchecked_err = validate_transfer(&stmt, None).unwrap_err();
            let checked_err = CheckedStealthTransfer::check(stmt).unwrap_err();
            assert_eq!(checked_err.to_string(), unchecked_err.to_string());
            assert!(
                checked_err.to_string().contains(expected),
                "expected `{expected}`, got: {checked_err:?}"
            );
        }

        // A structurally sound statement is checked, and the remaining checks still reject its missing balance proof.
        let stmt = statement(vec![input1], vec![out2], &[0]);
        let unchecked_err = validate_transfer(&stmt, None).unwrap_err();
        let checked = CheckedStealthTransfer::check(stmt).unwrap();
        let checked_err = validate_checked_transfer(&checked, None).unwrap_err();
        assert!(
            matches!(&checked_err, ResourceError::InvalidBalanceProof { details }
                if details.contains("Balance proof must be provided")),
            "unexpected error: {checked_err:?}"
        );
        assert_eq!(checked_err.to_string(), unchecked_err.to_string());
    }
}
