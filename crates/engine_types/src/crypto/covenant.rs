//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use ootle_byte_type::{ConvertFromByteType, FromByteType};
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{
    Amount,
    Hash32,
    crypto::{BalanceProofSignature, PedersenCommitmentBytes},
};

use crate::crypto::{commit_amount, messages, try_decode_to_signature};

const LOG_TARGET: &str = "tari::ootle::engine::crypto::covenant";

/// Verifies a covenant sub-balance proof (TIP-0006 Option A/C): that the value committed by `input_commitments` equals
/// the value committed by `output_commitments` plus the cleartext `revealed_amount`.
///
/// The signature is a Schnorr proof of knowledge of the discrete log of the reconstructed excess point with respect to
/// `G`, which holds only when the value (`H`) components cancel — i.e. when the partition conserves value up to the
/// declared `revealed_amount`. Confidential values are never exposed. Soundness against forged output values relies on
/// the per-output range proofs verified elsewhere in the transfer pipeline.
///
/// `revealed_amount` is the exact net cleartext outflow of the partition; the caller bounds it against the script's
/// permitted allowance. Amounts above `u64::MAX` are rejected by `commit_amount` below (the range-proof domain).
pub fn validate_covenant_balance_proof(
    condition_root: &Hash32,
    revealed_amount: Amount,
    input_commitments: &[PedersenCommitmentBytes],
    output_commitments: &[PedersenCommitmentBytes],
    signature: &BalanceProofSignature,
) -> bool {
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
        condition_root,
        &revealed_amount,
        input_commitments,
        output_commitments,
    );
    sig.verify_raw_uniform(&public_excess, &message)
}

/// Native points for one [`validate_covenant_balance_proof`] over a partition of `num_inputs` inputs and `num_outputs`
/// outputs. Each commitment is decompressed, folded into its side's aggregate and hashed into the challenge; the
/// revealed amount is committed against the basepoint; the signature is verified once.
pub fn covenant_balance_proof_native_points(num_inputs: usize, num_outputs: usize) -> u64 {
    use crate::limits::NativeExecutionPoints as P;
    /// The challenge's fixed preimage: excess, nonce and condition root (32 bytes each) and the revealed amount.
    const FIXED_CHALLENGE_BYTES: u64 = 3 * 32 + 16;
    let num_commitments = (num_inputs as u64).saturating_add(num_outputs as u64);
    let challenge_bytes = FIXED_CHALLENGE_BYTES.saturating_add(num_commitments.saturating_mul(32));
    P::PER_SCHNORR_VERIFY
        .saturating_add(P::PER_RISTRETTO_MUL_BASE)
        .saturating_add(P::PER_INPUT.saturating_mul(num_commitments))
        .saturating_add(P::PER_HASH)
        .saturating_add(P::PER_HASH_BYTE.saturating_mul(challenge_bytes))
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
