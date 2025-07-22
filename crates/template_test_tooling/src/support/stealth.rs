//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_common_types::types::PrivateKey;
use tari_crypto::{keys::SecretKey, ristretto::RistrettoPublicKey};
use tari_ootle_wallet_crypto::{stealth, MaskAndValue, UnblindedStatement};
use tari_template_lib::{
    models::{EncryptedData, StealthMintStatement, StealthOutputsStatement, StealthTransferStatement},
    types::Amount,
};

pub fn generate_stealth_output_statement<I: IntoIterator<Item = A>, A: Into<Amount>>(
    output_amounts: I,
    revealed_output_amount: A,
) -> (StealthOutputsStatement, Vec<PrivateKey>) {
    generate_stealth_statement_internal(
        &output_amounts.into_iter().map(Into::into).collect::<Vec<_>>(),
        revealed_output_amount.into(),
        None,
    )
}

pub fn generate_mint_statement<I: IntoIterator<Item = A>, A: Into<Amount>>(
    stealth_output_amounts: I,
    revealed_output_amount: A,
    view_key: Option<RistrettoPublicKey>,
) -> (StealthMintStatement, Vec<PrivateKey>) {
    let amounts = stealth_output_amounts.into_iter().map(Into::into).collect::<Vec<_>>();
    let (stmt, masks) = generate_stealth_statement_internal(&amounts, revealed_output_amount.into(), view_key);
    let stmt = stealth::create_mint_statement(stmt, &masks, &amounts).unwrap();
    (stmt, masks)
}

pub fn generate_stealth_statement_with_view_key<I: IntoIterator<Item = A>, A: Into<Amount>>(
    output_amounts: I,
    revealed_output_amount: Amount,
    view_key: &RistrettoPublicKey,
) -> (StealthOutputsStatement, Vec<PrivateKey>) {
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
) -> (StealthOutputsStatement, Vec<PrivateKey>) {
    let masks = output_amounts
        .iter()
        .map(|_| PrivateKey::random(&mut OsRng))
        .collect::<Vec<_>>();
    let output_statements = output_amounts
        .iter()
        .zip(&masks)
        .map(|(amount, mask)| UnblindedStatement {
            amount: *amount,
            mask: mask.clone(),
            sender_public_nonce: Default::default(),
            minimum_value_promise: 0,
            encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
            resource_view_key: view_key.clone(),
        })
        .collect::<Vec<_>>();

    let stmt = stealth::create_output_statement(&output_statements, revealed_output_amount).unwrap();
    (stmt, masks)
}

pub struct StealthUnblindedTransferData {
    pub output_masks: Vec<PrivateKey>,
    pub statement: StealthTransferStatement,
}

pub fn generate_transfer_data<O, A>(
    inputs: &[MaskAndValue],
    revealed_input_amount: A,
    output_amounts: O,
    revealed_output_amount: A,
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
    )
}

pub fn generate_transfer_data_with_view_key<I: IntoIterator<Item = A>, A: Into<Amount>>(
    inputs: &[MaskAndValue],
    revealed_input_amount: A,
    output_amounts: I,
    revealed_output_amount: A,
    view_key: &RistrettoPublicKey,
) -> StealthUnblindedTransferData {
    generate_transfer_data_internal(
        inputs,
        revealed_input_amount,
        output_amounts,
        revealed_output_amount,
        Some(view_key.clone()),
    )
}

fn generate_transfer_data_internal<I: IntoIterator<Item = A>, A: Into<Amount>>(
    inputs: &[MaskAndValue],
    revealed_input_amount: A,
    output_amounts: I,
    revealed_output_amount: A,
    view_key: Option<RistrettoPublicKey>,
) -> StealthUnblindedTransferData {
    let outputs = output_amounts
        .into_iter()
        .map(|a| {
            // If the amount is zero, we omit the output UTXO, therefore the mask is zero
            let amount = a.into();
            let output_mask = if amount.is_zero() {
                Default::default()
            } else {
                PrivateKey::random(&mut OsRng)
            };
            UnblindedStatement {
                amount,
                mask: output_mask,
                sender_public_nonce: Default::default(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: view_key.clone(),
            }
        })
        .collect::<Vec<_>>();

    let transfer = stealth::create_transfer_statement(
        inputs,
        &outputs,
        revealed_input_amount.into(),
        revealed_output_amount.into(),
    )
    .unwrap();

    StealthUnblindedTransferData {
        output_masks: outputs.into_iter().map(|m| m.mask).collect(),
        statement: transfer,
    }
}
