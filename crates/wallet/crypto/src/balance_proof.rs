//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead::OsRng;
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
    crypto::BalanceProofSignature,
    stealth::{StealthInputsStatement, StealthOutputsStatement},
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
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
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
) -> BalanceProofSignature {
    let secret_excess = agg_input_mask - agg_output_mask;
    if secret_excess == RistrettoSecretKey::default() {
        // This is a revealed only proof
        return BalanceProofSignature::zero();
    }
    let public_excess = RistrettoPublicKey::from_secret_key(&secret_excess);
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let message = messages::stealth_balance_proof64(&public_excess, &public_nonce, inputs_statement, outputs_statement);

    let sig = EngineSchnorrSignature::sign_raw_uniform(&secret_excess, nonce, &message).unwrap();
    sig.to_byte_type()
}

pub fn validate_balance_proof_signature(
    signature: &BalanceProofSignature,
    inputs_statement: &StealthInputsStatement,
    outputs_statement: &StealthOutputsStatement,
) -> bool {
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

    let Some(revealed_output_commit) =
        commit_amount(&RistrettoSecretKey::default(), outputs_statement.revealed_output_amount)
    else {
        warn!(target: LOG_TARGET, "Revealed output amount must be non-negative");
        return false;
    };

    let public_excess =
        agg_inputs + revealed_input_commit.as_public_key() - &agg_outputs - revealed_output_commit.as_public_key();

    let public_nonce = signature.public_nonce();
    let Ok(public_nonce) = public_nonce.try_from_byte_type() else {
        return false;
    };

    let message = messages::stealth_balance_proof64(&public_excess, &public_nonce, inputs_statement, outputs_statement);
    sig.verify_raw_uniform(&public_excess, &message)
}
