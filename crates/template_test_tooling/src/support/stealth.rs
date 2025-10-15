//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_engine_types::{
    crypto::{commit_amount, messages},
    ToByteType,
};
use tari_ootle_wallet_crypto::{
    stealth,
    MaskAndValue,
    UnblindedOutputWitness,
    UnblindedStealthInputWitness,
    UnblindedStealthOutputWitness,
};
use tari_template_lib::{
    models::{StealthOutputsStatement, StealthTransferStatement},
    prelude::{crypto::ValueKnowledgeProof, RistrettoPublicKeyBytes},
    types::{
        crypto::{StealthValueProof, UtxoTag},
        Amount,
        EncryptedData,
    },
};

pub fn generate_stealth_output_statement<I: IntoIterator<Item = A>, A: Into<Amount>>(
    output_amounts: I,
    revealed_output_amount: A,
) -> (StealthOutputsStatement, Vec<RistrettoSecretKey>) {
    generate_stealth_statement_internal(
        &output_amounts.into_iter().map(Into::into).collect::<Vec<_>>(),
        revealed_output_amount.into(),
        None,
    )
}

pub fn generate_mint_statement<I: IntoIterator<Item = A>, A: Into<Amount> + Copy>(
    stealth_output_amounts: I,
    revealed_output_amount: A,
    view_key: Option<&RistrettoPublicKey>,
    required_signer: RistrettoPublicKeyBytes,
) -> StealthUnblindedTransferData {
    let stealth_output_amounts = stealth_output_amounts.into_iter().map(Into::into).collect::<Vec<_>>();
    let total_revealed_inputs = stealth_output_amounts.iter().copied().sum::<Amount>() + revealed_output_amount.into();
    match view_key {
        Some(view_key) => generate_transfer_data_with_view_key(
            &[],
            total_revealed_inputs,
            stealth_output_amounts,
            revealed_output_amount.into(),
            view_key,
            required_signer,
        ),

        None => generate_transfer_data(
            &[],
            total_revealed_inputs,
            stealth_output_amounts,
            revealed_output_amount.into(),
            required_signer,
        ),
    }
}

pub fn generate_stealth_statement_with_view_key<I: IntoIterator<Item = A>, A: Into<Amount>>(
    output_amounts: I,
    revealed_output_amount: Amount,
    view_key: &RistrettoPublicKey,
) -> (StealthOutputsStatement, Vec<RistrettoSecretKey>) {
    generate_stealth_statement_internal(
        &output_amounts.into_iter().map(Into::into).collect::<Vec<_>>(),
        revealed_output_amount,
        Some(view_key.clone()),
    )
}

fn generate_stealth_statement_internal(
    output_amounts: &[Amount],
    revealed_output_amount: Amount,
    view_key: Option<RistrettoPublicKey>,
) -> (StealthOutputsStatement, Vec<RistrettoSecretKey>) {
    let masks = output_amounts
        .iter()
        .map(|_| RistrettoSecretKey::random(&mut OsRng))
        .collect::<Vec<_>>();
    let output_statements = output_amounts
        .iter()
        .zip(&masks)
        .map(|(amount, mask)| UnblindedStealthOutputWitness {
            witness: UnblindedOutputWitness {
                amount: *amount,
                mask: mask.clone(),
                sender_public_nonce: test_sender_public_nonce(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: view_key.clone(),
            },
            output_owner_public_key: RistrettoPublicKey::from_secret_key(mask),
            tag: UtxoTag::new(0),
        })
        .collect::<Vec<_>>();

    let stmt = stealth::create_outputs_statement(&output_statements, revealed_output_amount).unwrap();
    (stmt, masks)
}

pub struct StealthUnblindedTransferData {
    pub output_masks: Vec<RistrettoSecretKey>,
    pub statement: StealthTransferStatement,
}

pub fn generate_transfer_data<O, A>(
    inputs: &[MaskAndValue],
    revealed_input_amount: A,
    output_amounts: O,
    revealed_output_amount: A,
    required_signer: RistrettoPublicKeyBytes,
) -> StealthUnblindedTransferData
where
    O: IntoIterator<Item = A>,
    A: Into<Amount>,
{
    generate_transfer_data_internal(
        inputs,
        revealed_input_amount,
        output_amounts,
        revealed_output_amount,
        None,
        required_signer,
    )
}

pub fn generate_transfer_data_with_view_key<I: IntoIterator<Item = A>, A: Into<Amount>>(
    inputs: &[MaskAndValue],
    revealed_input_amount: A,
    output_amounts: I,
    revealed_output_amount: A,
    view_key: &RistrettoPublicKey,
    required_signer: RistrettoPublicKeyBytes,
) -> StealthUnblindedTransferData {
    generate_transfer_data_internal(
        inputs,
        revealed_input_amount,
        output_amounts,
        revealed_output_amount,
        Some(view_key.clone()),
        required_signer,
    )
}

/// Generates a non-zero test sender nonce keypair for testing purposes.
/// This is not secure (it is a hardcoded value) and should only be used for testing scenarios.
pub fn test_sender_nonce_keypair() -> (RistrettoSecretKey, RistrettoPublicKey) {
    let sender_nonce = RistrettoSecretKey::from(123);
    let sender_public_nonce = RistrettoPublicKey::from_secret_key(&sender_nonce);
    (sender_nonce, sender_public_nonce)
}
pub fn test_sender_public_nonce() -> RistrettoPublicKey {
    test_sender_nonce_keypair().1
}

fn generate_transfer_data_internal<I: IntoIterator<Item = A>, A: Into<Amount>>(
    inputs: &[MaskAndValue],
    revealed_input_amount: A,
    output_amounts: I,
    revealed_output_amount: A,
    view_key: Option<RistrettoPublicKey>,
    required_signer: RistrettoPublicKeyBytes,
) -> StealthUnblindedTransferData {
    let outputs = output_amounts
        .into_iter()
        .map(|a| {
            // If the amount is zero, we omit the output UTXO, therefore, the mask is zero
            let amount = a.into();
            let output_mask = if amount.is_zero() {
                Default::default()
            } else {
                RistrettoSecretKey::random(&mut OsRng)
            };
            // For testing purposes, we use the mask as the owner key
            let output_owner_public_key = RistrettoPublicKey::from_secret_key(&output_mask);
            let statement = UnblindedOutputWitness {
                amount,
                mask: output_mask,
                resource_view_key: view_key.clone(),
                // This is client/wallet on-chain data and not required for spending in tests
                sender_public_nonce: test_sender_public_nonce(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
            };

            UnblindedStealthOutputWitness {
                witness: statement,
                output_owner_public_key,
                tag: UtxoTag::new(0),
            }
        })
        .collect::<Vec<_>>();

    let inputs = inputs
        .iter()
        .map(|input| {
            let mask_and_value = input.clone();
            UnblindedStealthInputWitness {
                mask_and_value,
                // For testing purposes, we use the mask as the owner key
                owner_secret: input.mask.clone(),
                public_nonce: test_sender_public_nonce(),
            }
        })
        .collect::<Vec<_>>();

    let transfer = stealth::create_transfer_statement(
        &inputs,
        revealed_input_amount.into(),
        &outputs,
        revealed_output_amount.into(),
        required_signer,
    )
    .unwrap();

    StealthUnblindedTransferData {
        output_masks: outputs.into_iter().map(|m| m.witness.mask).collect(),
        statement: transfer,
    }
}

pub fn generate_value_proof_mask_knowledge(value: Amount, mask: &RistrettoSecretKey) -> StealthValueProof {
    assert!(value.is_positive(), "Value must be positive");
    let commitment = commit_amount(mask, value);
    let commitment_bytes = commitment.to_byte_type();
    let message = messages::value_proof_message(&commitment_bytes, &value);
    let sig = RistrettoSchnorr::sign(mask, message, &mut OsRng).expect("Signing cannot fail");

    StealthValueProof {
        value,
        knowledge_proof: ValueKnowledgeProof::Commitment {
            mask_knowledge_proof: sig.to_byte_type(),
        },
    }
}

pub fn generate_value_proof_elgamal(value: Amount, reveal_key: RistrettoPublicKeyBytes) -> StealthValueProof {
    assert!(value.is_positive(), "Value must be positive");
    StealthValueProof {
        value,
        knowledge_proof: ValueKnowledgeProof::ElgamalEncrypted { reveal_key },
    }
}
