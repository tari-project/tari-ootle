//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeSet;

use log::warn;
use ootle_byte_type::{ConvertFromByteType, FromByteType, ToByteType};
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey, pedersen::PedersenCommitment},
};
use tari_engine_types::{
    crypto::{commit_amount, messages},
    hashing::EngineSchnorrSignature,
};
use tari_template_lib_types::{
    Amount,
    Hash32,
    crypto::{BalanceProofSignature, PedersenCommitmentBytes},
    stealth::{CovenantBalanceClaim, StealthInputsStatement, StealthOutputsStatement},
};
use tari_utilities::ByteArrayError;

const LOG_TARGET: &str = "tari::wallet::crypto::balance_proof";

pub(crate) fn generate_confidential_balance_proof(
    input_mask: &RistrettoSecretKey,
    input_revealed_amount: &Amount,
    output_mask: Option<&RistrettoSecretKey>,
    change_mask: Option<&RistrettoSecretKey>,
    output_reveal_amount: &Amount,
) -> BalanceProofSignature {
    let secret_excess = input_mask -
        output_mask.unwrap_or(&RistrettoSecretKey::default()) -
        change_mask.unwrap_or(&RistrettoSecretKey::default());
    if secret_excess == RistrettoSecretKey::default() {
        // This is a revealed only proof
        return BalanceProofSignature::zero();
    }
    let excess = RistrettoPublicKey::from_secret_key(&secret_excess);
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let message =
        messages::confidential_withdraw64(&excess, &public_nonce, input_revealed_amount, output_reveal_amount);

    let sig = EngineSchnorrSignature::sign_raw_uniform(&secret_excess, nonce, &message).unwrap();
    sig.to_byte_type()
}

pub fn generate_stealth_balance_proof_signature(
    agg_input_mask: &RistrettoSecretKey,
    agg_output_mask: &RistrettoSecretKey,
    inputs_statement: &StealthInputsStatement,
    outputs_statement: &StealthOutputsStatement,
    covenant_claims: &[CovenantBalanceClaim],
) -> BalanceProofSignature {
    let secret_excess = agg_input_mask - agg_output_mask;
    if secret_excess == RistrettoSecretKey::default() {
        // This is a revealed only proof
        return BalanceProofSignature::zero();
    }
    let public_excess = RistrettoPublicKey::from_secret_key(&secret_excess);
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let message = messages::stealth_balance_proof64(
        &public_excess,
        &public_nonce,
        inputs_statement,
        outputs_statement,
        covenant_claims,
    );

    let sig = EngineSchnorrSignature::sign_raw_uniform(&secret_excess, nonce, &message).unwrap();
    sig.to_byte_type()
}

/// Signs a covenant sub-balance proof (TIP-0006) for one partition: the inputs and outputs sharing `condition_root`.
/// The key is the partition's aggregate mask difference (`agg_input_mask - agg_output_mask`); `revealed_amount` is the
/// exact net cleartext outflow (`Σ input values - Σ output values`) and must be non-negative. The verifier
/// reconstructs the same excess point from the public commitments and `revealed_amount`.
pub fn generate_covenant_balance_proof_signature(
    condition_root: &Hash32,
    agg_input_mask: &RistrettoSecretKey,
    agg_output_mask: &RistrettoSecretKey,
    revealed_amount: Amount,
    input_commitments: &[PedersenCommitmentBytes],
    output_commitments: &[PedersenCommitmentBytes],
) -> BalanceProofSignature {
    // Unlike the global balance proofs, there is no zero-excess sentinel: a covenant partition's aggregate mask
    // difference is non-trivial for distinct output masks, and a degenerate zero excess would yield the identity
    // point, which the verifier rejects — the correct fail-closed outcome.
    let secret_excess = agg_input_mask - agg_output_mask;
    let public_excess = RistrettoPublicKey::from_secret_key(&secret_excess);
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let message = messages::covenant_balance_proof64(
        &public_excess,
        &public_nonce,
        condition_root,
        &revealed_amount,
        input_commitments,
        output_commitments,
    );

    let sig = EngineSchnorrSignature::sign_raw_uniform(&secret_excess, nonce, &message).unwrap();
    sig.to_byte_type()
}

#[must_use]
pub fn validate_balance_proof_signature(
    signature: &BalanceProofSignature,
    inputs_statement: &StealthInputsStatement,
    outputs_statement: &StealthOutputsStatement,
    covenant_claims: &[CovenantBalanceClaim],
) -> bool {
    if signature.public_nonce().is_zero() || signature.signature().is_zero() {
        warn!(target: LOG_TARGET, "Balance proof public nonce and signature cannot be zero");
        return false;
    }

    let mut seen_inputs = BTreeSet::new();
    for input in &inputs_statement.inputs {
        if !seen_inputs.insert(input.commitment) {
            warn!(target: LOG_TARGET, "Duplicate commitment in transfer inputs");
            return false;
        }
    }

    if let Some(revealed) = outputs_statement.revealed_output {
        if !revealed.amount.is_positive() {
            warn!(target: LOG_TARGET, "Revealed output amount must be positive");
            return false;
        }
        if RistrettoPublicKey::convert_from_byte_type(&revealed.receiver).is_err() {
            warn!(target: LOG_TARGET, "Revealed output receiver is not a canonical public key");
            return false;
        }
    }

    let mut claimed_partitions = BTreeSet::new();
    for claim in covenant_claims {
        let index = claim.partition_input_index;
        if index as usize >= inputs_statement.inputs.len() {
            warn!(
                target: LOG_TARGET,
                "Covenant balance claim names out-of-bounds input index {index}"
            );
            return false;
        }
        if !claimed_partitions.insert(index) {
            warn!(
                target: LOG_TARGET,
                "Duplicate covenant balance claim for input index {index}"
            );
            return false;
        }
    }

    let mut seen_outputs = BTreeSet::new();
    for output in &outputs_statement.outputs {
        let commitment = *output.commitment();
        if seen_inputs.contains(&commitment) {
            warn!(
                target: LOG_TARGET,
                "Output commitment {commitment} duplicates an input commitment"
            );
            return false;
        }
        if !seen_outputs.insert(commitment) {
            warn!(target: LOG_TARGET, "Duplicate commitment in transfer outputs");
            return false;
        }
    }

    let Ok(sig) = EngineSchnorrSignature::convert_from_byte_type(signature) else {
        warn!(target: LOG_TARGET, "Malformed balance proof signature");
        return false;
    };

    let Ok(agg_outputs) = outputs_statement
        .outputs
        .iter()
        .try_fold(RistrettoPublicKey::default(), |acc, o| {
            let commit = PedersenCommitment::convert_from_byte_type(o.commitment())?;
            Ok::<_, ByteArrayError>(acc + commit.as_public_key())
        })
    else {
        warn!(target: LOG_TARGET, "Malformed commitment in transfer outputs");
        return false;
    };

    let Ok(agg_inputs) = inputs_statement
        .inputs
        .iter()
        .try_fold(RistrettoPublicKey::default(), |sum, input| {
            let commit = PedersenCommitment::convert_from_byte_type(&input.commitment)?;
            Ok::<_, ByteArrayError>(sum + commit.as_public_key())
        })
    else {
        warn!(target: LOG_TARGET, "Malformed commitment in transfer inputs");
        return false;
    };

    // We assume that the input amount is available and only check that the maths is correct. The engine is responsible
    // for checking that the input amount is actually available.
    let Some(revealed_input_commit) = commit_amount(&RistrettoSecretKey::default(), inputs_statement.revealed_amount)
    else {
        warn!(target: LOG_TARGET, "Revealed input amount must be non-negative");
        return false;
    };

    let Some(revealed_output_commit) = commit_amount(
        &RistrettoSecretKey::default(),
        outputs_statement.revealed_output_amount(),
    ) else {
        warn!(target: LOG_TARGET, "Revealed output amount must be non-negative");
        return false;
    };

    let public_excess =
        agg_inputs + revealed_input_commit.as_public_key() - &agg_outputs - revealed_output_commit.as_public_key();

    let public_nonce = signature.public_nonce();
    let Ok(public_nonce) = public_nonce.try_from_byte_type() else {
        return false;
    };

    let message = messages::stealth_balance_proof64(
        &public_excess,
        &public_nonce,
        inputs_statement,
        outputs_statement,
        covenant_claims,
    );
    sig.verify_raw_uniform(&public_excess, &message)
}

#[cfg(test)]
mod tests {
    use tari_crypto::keys::SecretKey;
    use tari_engine_types::crypto::commit_u64_amount;
    use tari_template_lib_types::{
        EncryptedData,
        crypto::{RangeProofBytes, RistrettoPublicKeyBytes, UtxoTag},
        stealth::{RevealedOutput, SpendAuthorization, SpendWitness, StealthInput, StealthUnspentOutput, UnspentOutput},
    };

    use super::*;

    fn sample_output(mask: &RistrettoSecretKey, amount: u64) -> StealthUnspentOutput {
        let commitment = commit_u64_amount(mask, amount).to_byte_type();
        let pk = RistrettoPublicKey::from_secret_key(mask).to_byte_type();
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
    fn validate_balance_proof_signature_rejects_commitment_collisions_invalid_receiver_and_invalid_claims() {
        let in_mask = RistrettoSecretKey::random(&mut rand::rng());
        let out_mask = RistrettoSecretKey::random(&mut rand::rng());
        let in_commit = commit_u64_amount(&in_mask, 100).to_byte_type();
        let out = sample_output(&out_mask, 100);

        let inputs_stmt = StealthInputsStatement {
            inputs: vec![StealthInput {
                commitment: in_commit,
                witness: SpendWitness::KeyPath,
            }],
            revealed_amount: Amount::zero(),
        };
        let outputs_stmt = StealthOutputsStatement {
            outputs: vec![out.clone()],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let sig = generate_stealth_balance_proof_signature(&in_mask, &out_mask, &inputs_stmt, &outputs_stmt, &[]);
        assert!(validate_balance_proof_signature(
            &sig,
            &inputs_stmt,
            &outputs_stmt,
            &[]
        ));

        // 1. Duplicate output commitments are rejected even when masks/commitments balance.
        let dup_in_mask = &out_mask + &out_mask;
        let dup_inputs_stmt = StealthInputsStatement {
            inputs: vec![StealthInput {
                commitment: commit_u64_amount(&dup_in_mask, 200).to_byte_type(),
                witness: SpendWitness::KeyPath,
            }],
            revealed_amount: Amount::zero(),
        };
        let dup_outputs_stmt = StealthOutputsStatement {
            outputs: vec![out.clone(), out.clone()],
            revealed_output: None,
            agg_range_proof: RangeProofBytes::empty(),
        };
        let dup_sig = generate_stealth_balance_proof_signature(
            &dup_in_mask,
            &dup_in_mask,
            &dup_inputs_stmt,
            &dup_outputs_stmt,
            &[],
        );
        assert!(!validate_balance_proof_signature(
            &dup_sig,
            &dup_inputs_stmt,
            &dup_outputs_stmt,
            &[]
        ));

        // 2. Output commitment duplicating an input commitment is rejected.
        let extra_mask = RistrettoSecretKey::random(&mut rand::rng());
        let colliding_in_mask = &out_mask + &extra_mask;
        let colliding_inputs_stmt = StealthInputsStatement {
            inputs: vec![
                StealthInput {
                    commitment: *out.commitment(),
                    witness: SpendWitness::KeyPath,
                },
                StealthInput {
                    commitment: commit_u64_amount(&extra_mask, 0).to_byte_type(),
                    witness: SpendWitness::KeyPath,
                },
            ],
            revealed_amount: Amount::zero(),
        };
        let colliding_sig = generate_stealth_balance_proof_signature(
            &colliding_in_mask,
            &out_mask,
            &colliding_inputs_stmt,
            &outputs_stmt,
            &[],
        );
        assert!(!validate_balance_proof_signature(
            &colliding_sig,
            &colliding_inputs_stmt,
            &outputs_stmt,
            &[]
        ));

        // 3. Non-canonical revealed output receiver is rejected.
        let mut bad_receiver_outputs = outputs_stmt.clone();
        bad_receiver_outputs.revealed_output = Some(RevealedOutput::new(
            Amount::from(50u64),
            RistrettoPublicKeyBytes::from([0xffu8; 32]),
        ));
        let mut bad_receiver_inputs = inputs_stmt.clone();
        bad_receiver_inputs.revealed_amount = Amount::from(50u64);
        let bad_receiver_sig = generate_stealth_balance_proof_signature(
            &in_mask,
            &out_mask,
            &bad_receiver_inputs,
            &bad_receiver_outputs,
            &[],
        );
        assert!(!validate_balance_proof_signature(
            &bad_receiver_sig,
            &bad_receiver_inputs,
            &bad_receiver_outputs,
            &[]
        ));

        // 4. Out-of-bounds or duplicate covenant balance claim partition indices are rejected.
        let oob_claim = [CovenantBalanceClaim {
            partition_input_index: 1,
            revealed_amount: Amount::zero(),
            signature: BalanceProofSignature::zero(),
        }];
        let oob_sig =
            generate_stealth_balance_proof_signature(&in_mask, &out_mask, &inputs_stmt, &outputs_stmt, &oob_claim);
        assert!(!validate_balance_proof_signature(
            &oob_sig,
            &inputs_stmt,
            &outputs_stmt,
            &oob_claim
        ));

        let dup_claims = [
            CovenantBalanceClaim {
                partition_input_index: 0,
                revealed_amount: Amount::zero(),
                signature: BalanceProofSignature::zero(),
            },
            CovenantBalanceClaim {
                partition_input_index: 0,
                revealed_amount: Amount::zero(),
                signature: BalanceProofSignature::zero(),
            },
        ];
        let dup_claim_sig =
            generate_stealth_balance_proof_signature(&in_mask, &out_mask, &inputs_stmt, &outputs_stmt, &dup_claims);
        assert!(!validate_balance_proof_signature(
            &dup_claim_sig,
            &inputs_stmt,
            &outputs_stmt,
            &dup_claims
        ));
    }
}
