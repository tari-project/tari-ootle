//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{crypto::messages, hashing::EngineSchnorrSignature, ToByteType};
use tari_template_lib::{
    models::{
        StealthMintBalanceProof,
        StealthMintStatement,
        StealthOutputsStatement,
        StealthTransferStatement,
        UnspentOutput,
    },
    types::Amount,
};

use crate::{
    balance_proof::generate_stealth_balance_proof_signature,
    bullet_proof::generate_extended_bullet_proof,
    error::ConfidentialProofError,
    viewable_balance_proof::create_viewable_balance_proof,
    MaskAndValue,
    UnblindedStatement,
    WalletCryptoError,
};

pub fn create_transfer_statement(
    inputs: &[MaskAndValue],
    output_statements: &[UnblindedStatement],
    revealed_input_amount: Amount,
    revealed_output_amount: Amount,
) -> Result<StealthTransferStatement, WalletCryptoError> {
    if revealed_input_amount.is_negative() {
        return Err(WalletCryptoError::InvalidArgument {
            name: "revealed_input_amount",
            details: format!("Revealed input amount must be non-negative: {revealed_input_amount}"),
        });
    }
    if revealed_output_amount.is_negative() {
        return Err(WalletCryptoError::InvalidArgument {
            name: "revealed_output_amount",
            details: format!("Revealed output amount must be non-negative: {revealed_output_amount}"),
        });
    }

    let (input_commitments, agg_input_mask) = inputs.iter().try_fold(
        (Vec::with_capacity(inputs.len()), RistrettoSecretKey::default()),
        |(mut commitments, agg_input), input| {
            let commitment = input
                .to_commitment()
                .ok_or_else(|| WalletCryptoError::InvalidArgument {
                    name: "input value",
                    details: format!("Input value {} must be non-negative", input.value),
                })?;
            commitments.push(commitment.to_byte_type());
            Ok::<_, WalletCryptoError>((commitments, agg_input + &input.mask))
        },
    )?;

    let agg_output_mask = output_statements
        .iter()
        .map(|output| &output.mask)
        .fold(RistrettoSecretKey::default(), |agg, mask| agg + mask);

    let balance_proof = generate_stealth_balance_proof_signature(
        &agg_input_mask,
        &agg_output_mask,
        &revealed_input_amount,
        &revealed_output_amount,
    );

    let outputs_statement = create_output_statement(output_statements, revealed_output_amount)?;

    Ok(StealthTransferStatement {
        inputs: input_commitments,
        outputs_statement,
        balance_proof,
    })
}

pub fn create_output_statement(
    output_statements: &[UnblindedStatement],
    revealed_output_amount: Amount,
) -> Result<StealthOutputsStatement, ConfidentialProofError> {
    let outputs = output_statements
        .iter()
        .map(|stmt| {
            let commitment = stmt.to_commitment().ok_or(ConfidentialProofError::NegativeAmount)?;
            Ok::<_, ConfidentialProofError>(UnspentOutput {
                commitment: commitment.to_byte_type(),
                sender_public_nonce: stmt.sender_public_nonce.to_byte_type(),
                encrypted_data: stmt.encrypted_data.clone(),
                minimum_value_promise: stmt.minimum_value_promise,
                viewable_balance_proof: stmt
                    .resource_view_key
                    .as_ref()
                    .map(|view_key| {
                        let amount = stmt.amount;
                        create_viewable_balance_proof(&stmt.mask, amount, &commitment, view_key)
                    })
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let output_range_proof = generate_extended_bullet_proof(output_statements)?;

    Ok(StealthOutputsStatement {
        outputs,
        revealed_output_amount,
        agg_range_proof: output_range_proof,
    })
}

pub fn create_mint_statement(
    output_statement: StealthOutputsStatement,
    masks: &[RistrettoSecretKey],
    amounts: &[Amount],
) -> Result<StealthMintStatement, ConfidentialProofError> {
    let total_amount =
        Amount::sum_from_positive(amounts.iter().copied()).ok_or(ConfidentialProofError::NegativeAmount)?;
    let private_excess = masks
        .iter()
        .fold(RistrettoSecretKey::default(), |excess, mask| excess + mask);

    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let public_excess = RistrettoPublicKey::from_secret_key(&private_excess);

    eprintln!(
        "Sign: public_excess: {public_excess}, total_amount: {total_amount} nonce: {}",
        public_nonce
    );
    let message = messages::stealth_mint64(&public_excess, &public_nonce, total_amount);

    let excess_signature = EngineSchnorrSignature::sign_raw_uniform(&private_excess, nonce, &message)
        .expect("WIDE_REDUCTION_LEN == 64, failure is a bug");

    Ok(StealthMintStatement {
        balance_proof: StealthMintBalanceProof {
            excess_signature: excess_signature.to_byte_type(),
            total_mint_amount: total_amount,
        },
        outputs_statement: output_statement,
    })
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;
    use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
    use tari_engine_types::stealth::validate_stealth_outputs_statement;
    use tari_template_lib::{models::EncryptedData, types::Amount};

    use super::*;

    fn create_valid_proof(amount: Amount, minimum_value_promise: u64) -> StealthOutputsStatement {
        let mask = RistrettoSecretKey::random(&mut OsRng);
        create_output_statement(
            &[UnblindedStatement {
                amount,
                minimum_value_promise,
                mask,
                sender_public_nonce: Default::default(),
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            }],
            Amount::zero(),
        )
        .unwrap()
    }

    #[test]
    fn it_is_valid_if_proof_is_valid() {
        let proof = create_valid_proof(100.into(), 0);
        validate_stealth_outputs_statement(&proof, None).unwrap();
    }

    #[test]
    fn it_is_invalid_if_minimum_value_changed() {
        let mut proof = create_valid_proof(100.into(), 100);
        proof.outputs[0].minimum_value_promise = 99;
        validate_stealth_outputs_statement(&proof, None).unwrap_err();
        proof.outputs[0].minimum_value_promise = 1000;
        validate_stealth_outputs_statement(&proof, None).unwrap_err();
    }
}
