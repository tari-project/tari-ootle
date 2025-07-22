//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::aead::OsRng;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{crypto::messages, hashing::EngineSchnorrSignature, ToByteType};
use tari_template_lib::prelude::{Amount, BalanceProofSignature};

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

pub(crate) fn generate_stealth_balance_proof_signature(
    agg_input_mask: &RistrettoSecretKey,
    agg_output_mask: &RistrettoSecretKey,
    revealed_input_amount: &Amount,
    revealed_output_amount: &Amount,
) -> BalanceProofSignature {
    let secret_excess = agg_input_mask - agg_output_mask;
    let excess = RistrettoPublicKey::from_secret_key(&secret_excess);
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let message = messages::stealth_transfer64(&excess, &public_nonce, revealed_input_amount, revealed_output_amount);

    let sig = EngineSchnorrSignature::sign_raw_uniform(&secret_excess, nonce, &message).unwrap();
    sig.to_byte_type()
}
