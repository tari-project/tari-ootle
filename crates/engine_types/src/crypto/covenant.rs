//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use ootle_byte_type::{ConvertFromByteType, FromByteType};
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{
    Amount,
    crypto::{BalanceProofSignature, PedersenCommitmentBytes},
    stealth::SpendCondition,
};

use crate::crypto::{commit_amount, messages, try_decode_to_signature};

const LOG_TARGET: &str = "tari::engine::crypto::covenant";

/// Verifies a covenant sub-balance proof (TIP-0006 Option A/C): that the value committed by `input_commitments` equals
/// the value committed by `output_commitments` plus the cleartext `revealed_amount`.
///
/// The signature is a Schnorr proof of knowledge of the discrete log of the reconstructed excess point with respect to
/// `G`, which holds only when the value (`H`) components cancel — i.e. when the partition conserves value up to the
/// declared `revealed_amount`. Confidential values are never exposed. Soundness against forged output values relies on
/// the per-output range proofs verified elsewhere in the transfer pipeline.
///
/// `revealed_amount` is the exact net cleartext outflow of the partition and must be non-negative; the caller bounds it
/// against the script's permitted allowance.
pub fn validate_covenant_balance_proof(
    condition: &SpendCondition,
    revealed_amount: Amount,
    input_commitments: &[PedersenCommitmentBytes],
    output_commitments: &[PedersenCommitmentBytes],
    signature: &BalanceProofSignature,
) -> bool {
    if revealed_amount.is_negative() {
        return false;
    }

    let Some(sig) = try_decode_to_signature(signature) else {
        warn!(target: LOG_TARGET, "Malformed covenant balance proof signature");
        return false;
    };

    let Some(agg_inputs) = aggregate_commitments(input_commitments) else {
        warn!(target: LOG_TARGET, "Malformed commitment in covenant inputs");
        return false;
    };
    let Some(agg_outputs) = aggregate_commitments(output_commitments) else {
        warn!(target: LOG_TARGET, "Malformed commitment in covenant outputs");
        return false;
    };

    let Some(revealed_commit) = commit_amount(&RistrettoSecretKey::default(), revealed_amount) else {
        return false;
    };

    let public_excess = agg_inputs - &agg_outputs - revealed_commit.as_public_key();

    let Ok(public_nonce) = signature.public_nonce().try_from_byte_type() else {
        return false;
    };

    let message = messages::covenant_balance_proof64(
        &public_excess,
        &public_nonce,
        condition,
        &revealed_amount,
        input_commitments,
        output_commitments,
    );
    sig.verify_raw_uniform(&public_excess, &message)
}

fn aggregate_commitments<'a, I: IntoIterator<Item = &'a PedersenCommitmentBytes>>(
    commitments: I,
) -> Option<RistrettoPublicKey> {
    commitments
        .into_iter()
        .try_fold(RistrettoPublicKey::default(), |acc, c| {
            let commitment = PedersenCommitment::convert_from_byte_type(c).ok()?;
            Some(acc + commitment.as_public_key())
        })
}
