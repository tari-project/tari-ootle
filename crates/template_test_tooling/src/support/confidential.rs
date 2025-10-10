//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_common_types::types::PrivateKey;
use tari_crypto::{
    keys::SecretKey,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey},
};
use tari_engine_types::crypto::commit_amount_checked;
use tari_ootle_wallet_crypto::{confidential, MaskAndValue, UnblindedOutputStatement};
use tari_template_lib::{
    models::{ConfidentialOutputStatement, ConfidentialWithdrawProof},
    types::{Amount, EncryptedData},
};

pub fn generate_confidential_output_statement<A: Into<Amount>>(
    output_amount: A,
    change: Option<A>,
) -> (ConfidentialOutputStatement, PrivateKey, Option<PrivateKey>) {
    generate_confidential_proof_internal(output_amount.into(), change.map(Into::into), None)
}

pub fn generate_confidential_proof_with_view_key<A: Into<Amount>>(
    output_amount: A,
    change: Option<A>,
    view_key: &RistrettoPublicKey,
) -> (ConfidentialOutputStatement, PrivateKey, Option<PrivateKey>) {
    generate_confidential_proof_internal(output_amount.into(), change.map(Into::into), Some(view_key.clone()))
}

fn generate_confidential_proof_internal(
    output_amount: Amount,
    change: Option<Amount>,
    view_key: Option<RistrettoPublicKey>,
) -> (ConfidentialOutputStatement, PrivateKey, Option<PrivateKey>) {
    let mask = PrivateKey::random(&mut OsRng);
    let output_statement = UnblindedOutputStatement {
        amount: output_amount,
        mask: mask.clone(),
        sender_public_nonce: Default::default(),
        minimum_value_promise: 0,
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        resource_view_key: view_key.clone(),
    };

    let change_mask = PrivateKey::random(&mut OsRng);
    let change_statement = change.map(|amount| UnblindedOutputStatement {
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
    output_amount: A,
    change_amount: Option<A>,
    revealed_amount: A,
) -> ConfidentialWithdrawProofOutput {
    let output_amount = output_amount.into();
    let change_amount = change_amount.map(|a| a.into());
    let revealed_amount = revealed_amount.into();

    let total_amount = output_amount + change_amount.unwrap_or_else(Amount::zero) + revealed_amount;

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
    inputs: &[(PrivateKey, Amount)],
    input_revealed_amount: A,
    output_amount: A,
    change_amount: Option<A>,
    revealed_output_amount: A,
) -> ConfidentialWithdrawProofOutput {
    generate_withdraw_proof_internal(
        inputs,
        input_revealed_amount.into(),
        output_amount.into(),
        change_amount.map(Into::into),
        revealed_output_amount.into(),
        None,
    )
}

pub fn generate_withdraw_proof_with_view_key<A: Into<Amount>>(
    input_mask: &PrivateKey,
    input_value: A,
    output_amount: A,
    change_amount: Option<A>,
    revealed_amount: A,
    view_key: &RistrettoPublicKey,
) -> ConfidentialWithdrawProofOutput {
    generate_withdraw_proof_internal(
        &[(input_mask.clone(), input_value.into())],
        Amount::zero(),
        output_amount.into(),
        change_amount.map(Into::into),
        revealed_amount.into(),
        Some(view_key.clone()),
    )
}

fn generate_withdraw_proof_internal(
    inputs: &[(PrivateKey, Amount)],
    input_revealed_amount: Amount,
    output_amount: Amount,
    change_amount: Option<Amount>,
    revealed_output_amount: Amount,
    view_key: Option<RistrettoPublicKey>,
) -> ConfidentialWithdrawProofOutput {
    // If the amount is zero, we omit the output UTXO, therefore the mask is zero
    let output_mask = if output_amount.is_zero() {
        Default::default()
    } else {
        PrivateKey::random(&mut OsRng)
    };
    let change_mask = change_amount.map(|_| PrivateKey::random(&mut OsRng));

    let output_proof = UnblindedOutputStatement {
        amount: output_amount,
        mask: output_mask.clone(),
        sender_public_nonce: Default::default(),
        minimum_value_promise: 0,
        encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
        resource_view_key: view_key.clone(),
    };
    let change_proof = change_amount.map(|amount| UnblindedOutputStatement {
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
