//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_common_types::types::PrivateKey;
use tari_crypto::{
    keys::SecretKey,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey},
};
use tari_engine_types::crypto::commit_amount_checked;
use tari_ootle_wallet_crypto::{confidential, MaskAndValue, OutputWitness};
use tari_template_lib::types::{
    confidential::{ConfidentialOutputStatement, ConfidentialWithdrawProof},
    Amount,
    EncryptedData,
};

pub fn generate_confidential_output_statement(
    output_amount: u64,
    change: Option<u64>,
) -> (ConfidentialOutputStatement, PrivateKey, Option<PrivateKey>) {
    generate_confidential_proof_internal(output_amount, change, None)
}

pub fn generate_confidential_proof_with_view_key(
    output_amount: u64,
    change: Option<u64>,
    view_key: &RistrettoPublicKey,
) -> (ConfidentialOutputStatement, PrivateKey, Option<PrivateKey>) {
    generate_confidential_proof_internal(output_amount, change, Some(view_key.clone()))
}

fn generate_confidential_proof_internal(
    output_amount: u64,
    change: Option<u64>,
    view_key: Option<RistrettoPublicKey>,
) -> (ConfidentialOutputStatement, PrivateKey, Option<PrivateKey>) {
    let mask = PrivateKey::random(&mut OsRng);
    let output_statement = OutputWitness {
        amount: output_amount,
        mask: mask.clone(),
        sender_public_nonce: Default::default(),
        minimum_value_promise: 0,
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        resource_view_key: view_key.clone(),
    };

    let change_mask = PrivateKey::random(&mut OsRng);
    let change_statement = change.map(|amount| OutputWitness {
        amount,
        mask: change_mask.clone(),
        sender_public_nonce: Default::default(),
        minimum_value_promise: 0,
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        resource_view_key: view_key,
    });

    let proof = confidential::create_output_statement(
        Some(&output_statement),
        Amount::zero(),
        change_statement.as_ref(),
        Amount::zero(),
    )
    .unwrap();
    (proof, mask, change.map(|_| change_mask))
}

pub struct ConfidentialWithdrawProofOutput {
    pub output_mask: PrivateKey,
    pub change_mask: Option<PrivateKey>,
    pub proof: ConfidentialWithdrawProof,
}

impl ConfidentialWithdrawProofOutput {
    pub fn to_commitment_for_output(&self, amount: Amount) -> Option<PedersenCommitment> {
        commit_amount_checked(&self.output_mask, amount)
    }
}

pub fn generate_withdraw_proof<A: Into<Amount>>(
    input_mask: &PrivateKey,
    output_amount: u64,
    change_amount: Option<u64>,
    revealed_amount: A,
) -> ConfidentialWithdrawProofOutput {
    let revealed_amount = revealed_amount.into();

    let total_amount = output_amount +
        change_amount.unwrap_or(0) +
        revealed_amount.to_u64_checked().expect(
            "Revealed amount is too large to fit in u64 when generating withdraw proof. This is due to a current \
             limitation of the test tooling.",
        );

    generate_withdraw_proof_internal(
        &[(input_mask.clone(), total_amount)],
        Amount::zero(),
        output_amount,
        change_amount,
        revealed_amount,
        None,
    )
}

pub fn generate_withdraw_proof_with_inputs<A: Into<Amount>>(
    inputs: &[(PrivateKey, u64)],
    input_revealed_amount: A,
    output_amount: u64,
    change_amount: Option<u64>,
    revealed_output_amount: A,
) -> ConfidentialWithdrawProofOutput {
    generate_withdraw_proof_internal(
        inputs,
        input_revealed_amount.into(),
        output_amount,
        change_amount,
        revealed_output_amount.into(),
        None,
    )
}

pub fn generate_withdraw_proof_with_view_key<A: Into<Amount>>(
    input_mask: &PrivateKey,
    input_value: u64,
    output_amount: u64,
    change_amount: Option<u64>,
    revealed_amount: A,
    view_key: &RistrettoPublicKey,
) -> ConfidentialWithdrawProofOutput {
    generate_withdraw_proof_internal(
        &[(input_mask.clone(), input_value)],
        Amount::zero(),
        output_amount,
        change_amount,
        revealed_amount.into(),
        Some(view_key.clone()),
    )
}

fn generate_withdraw_proof_internal(
    inputs: &[(PrivateKey, u64)],
    input_revealed_amount: Amount,
    output_amount: u64,
    change_amount: Option<u64>,
    revealed_output_amount: Amount,
    view_key: Option<RistrettoPublicKey>,
) -> ConfidentialWithdrawProofOutput {
    // If the amount is zero, we omit the output UTXO, therefore the mask is zero
    let output_mask = if output_amount == 0 {
        Default::default()
    } else {
        PrivateKey::random(&mut OsRng)
    };
    let change_mask = change_amount.map(|_| PrivateKey::random(&mut OsRng));

    let output_proof = OutputWitness {
        amount: output_amount,
        mask: output_mask.clone(),
        sender_public_nonce: Default::default(),
        minimum_value_promise: 0,
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        resource_view_key: view_key.clone(),
    };
    let change_proof = change_amount.map(|amount| OutputWitness {
        amount,
        mask: change_mask.clone().unwrap(),
        sender_public_nonce: Default::default(),
        minimum_value_promise: 0,
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        resource_view_key: view_key,
    });

    let proof = confidential::create_withdraw_proof(
        &inputs
            .iter()
            .map(|(mask, amount)| MaskAndValue {
                value: *amount,
                mask: mask.clone(),
            })
            .collect::<Vec<_>>(),
        input_revealed_amount,
        Some(&output_proof),
        revealed_output_amount,
        change_proof.as_ref(),
        Amount::zero(),
    )
    .unwrap();

    ConfidentialWithdrawProofOutput {
        output_mask,
        change_mask,
        proof,
    }
}
