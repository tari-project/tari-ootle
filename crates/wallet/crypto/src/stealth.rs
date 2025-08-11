//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::ToByteType;
use tari_template_lib::{
    models::{
        StealthInput,
        StealthInputsStatement,
        StealthOutputsStatement,
        StealthTransferStatement,
        StealthUnspentOutput,
        UnspentOutput,
    },
    types::Amount,
};

use crate::{
    balance_proof::{generate_stealth_balance_proof_signature, generate_stealth_owner_proof_signature},
    bullet_proof::generate_extended_bullet_proof,
    error::ConfidentialProofError,
    viewable_balance_proof::create_viewable_balance_proof,
    UnblindedStealthInputStatement,
    UnblindedStealthOutputStatement,
    WalletCryptoError,
};

pub fn create_transfer_statement(
    inputs: &[UnblindedStealthInputStatement],
    revealed_input_amount: Amount,
    output_statements: &[UnblindedStealthOutputStatement],
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

    let (inputs_to_spend, agg_input_mask) = inputs.iter().try_fold(
        (Vec::with_capacity(inputs.len()), RistrettoSecretKey::default()),
        |(mut inputs, agg_input), input| {
            let commitment =
                input
                    .mask_and_value
                    .to_commitment()
                    .ok_or_else(|| WalletCryptoError::InvalidArgument {
                        name: "input value",
                        details: format!("Input value {} must be non-negative", input.mask_and_value.value),
                    })?;

            let signature = generate_stealth_owner_proof_signature(
                &input.owner_secret,
                &commitment.to_byte_type(),
                &input.public_nonce.to_byte_type(),
            );
            inputs.push(StealthInput {
                commitment: commitment.to_byte_type(),
                owner_proof: signature,
            });
            Ok::<_, WalletCryptoError>((inputs, agg_input + &input.mask_and_value.mask))
        },
    )?;

    let agg_output_mask = output_statements
        .iter()
        .map(|stmt| &stmt.statement.mask)
        .fold(RistrettoSecretKey::default(), |agg, mask| agg + mask);

    let balance_proof = generate_stealth_balance_proof_signature(
        &agg_input_mask,
        &agg_output_mask,
        &revealed_input_amount,
        &revealed_output_amount,
    );

    let outputs_statement = create_output_statement(output_statements, revealed_output_amount)?;

    Ok(StealthTransferStatement {
        inputs_statement: StealthInputsStatement {
            inputs: inputs_to_spend,
            revealed_amount: revealed_input_amount,
        },
        outputs_statement,
        balance_proof,
    })
}

pub fn create_output_statement(
    output_statements: &[UnblindedStealthOutputStatement],
    revealed_output_amount: Amount,
) -> Result<StealthOutputsStatement, ConfidentialProofError> {
    let outputs = output_statements
        .iter()
        .map(|output_stmt| {
            let unblinded_stmt = &output_stmt.statement;
            let commitment = output_stmt
                .statement
                .to_commitment()
                .ok_or(ConfidentialProofError::NegativeAmount)?;
            let output = UnspentOutput {
                commitment: commitment.to_byte_type(),
                sender_public_nonce: unblinded_stmt.sender_public_nonce.to_byte_type(),
                encrypted_data: unblinded_stmt.encrypted_data.clone(),
                minimum_value_promise: unblinded_stmt.minimum_value_promise,
                viewable_balance_proof: unblinded_stmt
                    .resource_view_key
                    .as_ref()
                    .map(|view_key| {
                        let amount = unblinded_stmt.amount;
                        create_viewable_balance_proof(&unblinded_stmt.mask, amount, &commitment, view_key)
                    })
                    .transpose()?,
            };

            Ok::<_, ConfidentialProofError>(StealthUnspentOutput {
                output,
                owner_public_key: output_stmt.output_owner_public_key.to_byte_type(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let output_range_proof = generate_extended_bullet_proof(output_statements.iter().map(|o| &o.statement))?;

    Ok(StealthOutputsStatement {
        outputs,
        revealed_output_amount,
        agg_range_proof: output_range_proof,
    })
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;
    use tari_crypto::{
        keys::SecretKey,
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };
    use tari_engine_types::stealth::validate_stealth_outputs_statement;
    use tari_template_lib::{models::EncryptedData, types::Amount};

    use super::*;
    use crate::UnblindedOutputStatement;

    fn create_valid_proof(amount: Amount, minimum_value_promise: u64) -> StealthOutputsStatement {
        let mask = RistrettoSecretKey::random(&mut OsRng);
        create_output_statement(
            &[UnblindedStealthOutputStatement {
                statement: UnblindedOutputStatement {
                    amount,
                    minimum_value_promise,
                    mask,
                    sender_public_nonce: Default::default(),
                    encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                    resource_view_key: None,
                },
                output_owner_public_key: RistrettoPublicKey::default(),
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
        proof.outputs[0].output.minimum_value_promise = 99;
        validate_stealth_outputs_statement(&proof, None).unwrap_err();
        proof.outputs[0].output.minimum_value_promise = 1000;
        validate_stealth_outputs_statement(&proof, None).unwrap_err();
    }
}
