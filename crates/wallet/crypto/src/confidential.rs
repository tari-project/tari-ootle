//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
use tari_engine_types::{crypto::commit_u64_amount, ToByteType};
use tari_template_lib::{
    models::{ConfidentialOutputStatement, ConfidentialWithdrawProof, UnspentOutput},
    types::{crypto::RistrettoPublicKeyBytes, Amount},
};

use crate::{
    balance_proof::generate_confidential_balance_proof,
    bullet_proof::generate_extended_bullet_proof,
    error::ConfidentialProofError,
    viewable_balance_proof::create_viewable_balance_proof,
    MaskAndValue,
    UnblindedOutputWitness,
    WalletCryptoError,
};

pub fn create_withdraw_proof(
    inputs: &[MaskAndValue],
    input_revealed_amount: Amount,
    output_statement: Option<&UnblindedOutputWitness>,
    output_revealed_amount: Amount,
    change_statement: Option<&UnblindedOutputWitness>,
    change_revealed_amount: Amount,
) -> Result<ConfidentialWithdrawProof, WalletCryptoError> {
    let output_proof = create_output_statement(
        output_statement,
        output_revealed_amount,
        change_statement,
        change_revealed_amount,
    )?;
    let (input_commitments, agg_input_mask) = inputs.iter().try_fold(
        (Vec::with_capacity(inputs.len()), RistrettoSecretKey::default()),
        |(mut commitments, agg_input), input| {
            let commitment = commit_u64_amount(&input.mask, input.value);
            commitments.push(commitment.to_byte_type());
            Ok::<_, WalletCryptoError>((commitments, agg_input + &input.mask))
        },
    )?;

    let output_revealed_amount = output_proof.output_revealed_amount + output_proof.change_revealed_amount;
    let balance_proof = generate_confidential_balance_proof(
        &agg_input_mask,
        &input_revealed_amount,
        output_statement.as_ref().map(|o| &o.mask),
        change_statement.as_ref().map(|ch| &ch.mask),
        &output_revealed_amount,
    );

    let output_statement = output_proof.output;
    let change_statement = output_proof.change_statement;

    Ok(ConfidentialWithdrawProof {
        inputs: input_commitments,
        input_revealed_amount,
        output_proof: ConfidentialOutputStatement {
            output: output_statement,
            change_statement,
            range_proof: output_proof.range_proof,
            output_revealed_amount: output_proof.output_revealed_amount,
            change_revealed_amount: output_proof.change_revealed_amount,
        },
        balance_proof,
    })
}

pub fn create_output_statement(
    output_statement: Option<&UnblindedOutputWitness>,
    output_revealed_amount: Amount,
    change_statement: Option<&UnblindedOutputWitness>,
    change_revealed_amount: Amount,
) -> Result<ConfidentialOutputStatement, ConfidentialProofError> {
    let proof_change_statement = change_statement
        .as_ref()
        .map(|stmt| -> Result<_, ConfidentialProofError> {
            let change_commitment = stmt.to_commitment();
            Ok(UnspentOutput {
                commitment: change_commitment.to_byte_type(),
                sender_public_nonce: RistrettoPublicKeyBytes::from_bytes(stmt.sender_public_nonce.as_bytes())
                    .expect("[generate_confidential_proof] change nonce"),
                encrypted_data: stmt.encrypted_data.clone(),
                minimum_value_promise: stmt.minimum_value_promise,
                viewable_balance_proof: stmt
                    .resource_view_key
                    .as_ref()
                    .map(|view_key| {
                        create_viewable_balance_proof(&stmt.mask, stmt.amount, &change_commitment, view_key)
                    })
                    .transpose()?,
            })
        })
        .transpose()?;
    let confidential_output_value = output_statement.as_ref().map(|o| o.amount).unwrap_or_default();

    let proof_output_statement = output_statement
        .as_ref()
        .map(|stmt| {
            let commitment = stmt.to_commitment();
            Ok::<_, ConfidentialProofError>(UnspentOutput {
                commitment: commitment.to_byte_type(),
                sender_public_nonce: stmt.sender_public_nonce.to_byte_type(),
                encrypted_data: stmt.encrypted_data.clone(),
                minimum_value_promise: stmt.minimum_value_promise,
                viewable_balance_proof: stmt
                    .resource_view_key
                    .as_ref()
                    .map(|view_key| {
                        create_viewable_balance_proof(&stmt.mask, confidential_output_value, &commitment, view_key)
                    })
                    .transpose()?,
            })
        })
        .transpose()?;

    let output_range_proof = generate_extended_bullet_proof(output_statement.into_iter().chain(change_statement))?;

    Ok(ConfidentialOutputStatement {
        output: proof_output_statement,
        change_statement: proof_change_statement,
        range_proof: output_range_proof,
        output_revealed_amount,
        change_revealed_amount,
    })
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;
    use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
    use tari_engine_types::confidential::validate_confidential_statement;
    use tari_template_lib::types::EncryptedData;

    use super::*;

    fn create_valid_proof(amount: u64, minimum_value_promise: u64) -> ConfidentialOutputStatement {
        let mask = RistrettoSecretKey::random(&mut OsRng);
        create_output_statement(
            Some(&UnblindedOutputWitness {
                amount,
                minimum_value_promise,
                mask,
                sender_public_nonce: Default::default(),
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            }),
            Default::default(),
            None,
            Default::default(),
        )
        .unwrap()
    }

    #[test]
    fn it_is_valid_if_proof_is_valid() {
        let proof = create_valid_proof(100, 0);
        validate_confidential_statement(&proof, None).unwrap();
    }

    #[test]
    fn it_is_invalid_if_minimum_value_changed() {
        let mut proof = create_valid_proof(100, 100);
        proof.output.as_mut().unwrap().minimum_value_promise = 99;
        validate_confidential_statement(&proof, None).unwrap_err();
        proof.output.as_mut().unwrap().minimum_value_promise = 1000;
        validate_confidential_statement(&proof, None).unwrap_err();
    }
}
