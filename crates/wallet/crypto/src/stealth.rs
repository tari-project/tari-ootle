//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_template_lib_types::{
    Amount,
    stealth::{
        StealthInput,
        StealthInputsStatement,
        StealthOutputsStatement,
        StealthTransferStatement,
        StealthUnspentOutput,
        UnspentOutput,
    },
};

use crate::{
    StealthInputWitness,
    StealthOutputWitness,
    WalletCryptoError,
    balance_proof::generate_stealth_balance_proof_signature,
    bullet_proof::generate_extended_bullet_proof,
    error::StealthProofError,
    viewable_balance_proof::generate_elgamal_viewable_balance_proof,
};

pub fn create_transfer_statement<'a, Inputs, Outputs>(
    inputs: Inputs,
    revealed_input_amount: Amount,
    output_statements: Outputs,
    revealed_output_amount: Amount,
) -> Result<StealthTransferStatement, WalletCryptoError>
where
    Inputs: IntoIterator<Item = StealthInputWitness>,
    Inputs::IntoIter: ExactSizeIterator,
    Outputs: IntoIterator<Item = &'a StealthOutputWitness> + Clone,
    Outputs::IntoIter: ExactSizeIterator,
{
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

    let mut inputs = inputs.into_iter();
    let num_inputs = inputs.len();

    let outputs_statement = create_outputs_statement(output_statements.clone(), revealed_output_amount)?;
    let output_statements = output_statements.into_iter();
    let num_outputs = output_statements.len();

    let (inputs_to_spend, agg_input_mask) = inputs.try_fold(
        (Vec::with_capacity(num_inputs), RistrettoSecretKey::default()),
        |(mut inputs, agg_input), input| {
            inputs.push(StealthInput {
                commitment: input.mask_and_value.to_commitment().to_byte_type(),
            });
            Ok::<_, WalletCryptoError>((inputs, agg_input + &input.mask_and_value.mask))
        },
    )?;

    let agg_output_mask = output_statements
        .map(|stmt| &stmt.witness.mask)
        .fold(RistrettoSecretKey::default(), |agg, mask| agg + mask);

    let inputs_statement = StealthInputsStatement {
        inputs: inputs_to_spend.clone(),
        revealed_amount: revealed_input_amount,
    };

    let requires_balance_proof = num_inputs > 0 || num_outputs > 0;
    let balance_proof = requires_balance_proof.then(|| {
        generate_stealth_balance_proof_signature(
            &agg_input_mask,
            &agg_output_mask,
            &inputs_statement,
            &outputs_statement,
        )
    });

    Ok(StealthTransferStatement {
        inputs_statement: StealthInputsStatement {
            inputs: inputs_to_spend,
            revealed_amount: revealed_input_amount,
        },
        outputs_statement,
        balance_proof,
    })
}

pub fn create_outputs_statement<'a, Outputs: IntoIterator<Item = &'a StealthOutputWitness> + Clone>(
    output_statements: Outputs,
    revealed_output_amount: Amount,
) -> Result<StealthOutputsStatement, StealthProofError> {
    let outputs = output_statements
        .clone()
        .into_iter()
        .map(|output_stmt| {
            let unblinded_stmt = &output_stmt.witness;
            let commitment = output_stmt.witness.to_commitment();
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
                        generate_elgamal_viewable_balance_proof(&unblinded_stmt.mask, amount, &commitment, view_key)
                    })
                    .transpose()?,
            };

            Ok::<_, StealthProofError>(StealthUnspentOutput {
                output,
                spend_condition: output_stmt.spend_condition.clone(),
                tag: output_stmt.tag,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let output_range_proof = generate_extended_bullet_proof(output_statements.into_iter().map(|o| &o.witness))?;

    Ok(StealthOutputsStatement {
        outputs,
        revealed_output_amount,
        agg_range_proof: output_range_proof,
    })
}

#[cfg(test)]
mod tests {
    use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
    use tari_engine_types::stealth::validate_stealth_outputs_statement;
    use tari_template_lib_types::{
        Amount,
        EncryptedData,
        crypto::{RistrettoPublicKeyBytes, UtxoTag},
        stealth::SpendCondition,
    };

    use super::*;
    use crate::OutputWitness;

    fn create_valid_proof(amount: u64, minimum_value_promise: u64) -> StealthOutputsStatement {
        let mask = RistrettoSecretKey::random(&mut rand::rng());
        create_outputs_statement(
            &[StealthOutputWitness {
                witness: OutputWitness {
                    amount,
                    minimum_value_promise,
                    mask,
                    sender_public_nonce: Default::default(),
                    encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                    resource_view_key: None,
                },
                spend_condition: SpendCondition::Signed(RistrettoPublicKeyBytes::default()),
                tag: UtxoTag::new(0),
            }],
            Amount::zero(),
        )
        .unwrap()
    }

    #[test]
    fn it_is_valid_if_proof_is_valid() {
        let proof = create_valid_proof(100, 0);
        validate_stealth_outputs_statement(&proof, None).unwrap();
    }

    #[test]
    fn it_is_invalid_if_minimum_value_changed() {
        let mut proof = create_valid_proof(100, 100);
        proof.outputs[0].output.minimum_value_promise = 99;
        validate_stealth_outputs_statement(&proof, None).unwrap_err();
        proof.outputs[0].output.minimum_value_promise = 1000;
        validate_stealth_outputs_statement(&proof, None).unwrap_err();
    }
}
