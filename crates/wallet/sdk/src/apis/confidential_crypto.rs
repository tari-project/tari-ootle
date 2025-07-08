//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use tari_common_types::types::PrivateKey;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::{
    confidential::{ConfidentialOutput, ElgamalVerifiableBalance, ValueLookupTable},
    FromByteType,
};
use tari_ootle_wallet_crypto::{
    create_confidential_output_statement,
    create_withdraw_proof,
    encrypt_value_and_mask,
    extract_value_and_mask,
    kdfs,
    unblind_output,
    ConfidentialOutputMaskAndValue,
    ConfidentialProofError,
    ConfidentialProofStatement,
    WalletCryptoError,
};
use tari_template_lib::{
    models::{ConfidentialOutputStatement, ConfidentialWithdrawProof, EncryptedData},
    prelude::PedersenCommitmentBytes,
    types::Amount,
};

pub struct ConfidentialCryptoApi;

impl ConfidentialCryptoApi {
    pub(crate) fn new() -> Self {
        Self
    }

    pub fn derive_encrypted_data_key_for_receiver(
        &self,
        public_nonce: &RistrettoPublicKey,
        private_key: &PrivateKey,
    ) -> PrivateKey {
        kdfs::encrypted_data_dh_kdf_aead(private_key, public_nonce)
    }

    pub fn generate_withdraw_proof<A: Into<Amount>>(
        &self,
        inputs: &[ConfidentialOutputMaskAndValue],
        input_revealed_amount: A,
        output_statement: Option<&ConfidentialProofStatement>,
        output_revealed_amount: A,
        change_statement: Option<&ConfidentialProofStatement>,
        change_revealed_amount: A,
    ) -> Result<ConfidentialWithdrawProof, ConfidentialCryptoApiError> {
        let proof = create_withdraw_proof(
            inputs,
            input_revealed_amount
                .into()
                .non_negative_checked()
                .ok_or(ConfidentialCryptoApiError::NegativeAmount)?,
            output_statement,
            output_revealed_amount
                .into()
                .non_negative_checked()
                .ok_or(ConfidentialCryptoApiError::NegativeAmount)?,
            change_statement,
            change_revealed_amount
                .into()
                .non_negative_checked()
                .ok_or(ConfidentialCryptoApiError::NegativeAmount)?,
        )?;
        Ok(proof)
    }

    pub fn encrypt_value_and_mask(
        &self,
        amount: u64,
        mask: &PrivateKey,
        public_nonce: &RistrettoPublicKey,
        secret: &PrivateKey,
    ) -> Result<EncryptedData, ConfidentialCryptoApiError> {
        let data = encrypt_value_and_mask(amount, mask, public_nonce, secret)?;
        Ok(data)
    }

    pub fn extract_value_and_mask(
        &self,
        encryption_key: &PrivateKey,
        commitment: &PedersenCommitmentBytes,
        encrypted_data: &EncryptedData,
    ) -> Result<(u64, PrivateKey), ConfidentialCryptoApiError> {
        let value_and_mask = extract_value_and_mask(encryption_key, commitment, encrypted_data)?;
        Ok(value_and_mask)
    }

    pub fn generate_output_proof<A: Into<Amount>>(
        &self,
        statement: &ConfidentialProofStatement,
        revealed_amount: A,
    ) -> Result<ConfidentialOutputStatement, ConfidentialCryptoApiError> {
        let proof = create_confidential_output_statement(
            Some(statement).filter(|s| !s.amount.is_zero()),
            revealed_amount.into(),
            None,
            Amount::zero(),
        )?;
        Ok(proof)
    }

    pub fn unblind_output(
        &self,
        output_commitment: &PedersenCommitmentBytes,
        output_encrypted_value: &EncryptedData,
        claim_secret: &PrivateKey,
        reciprocal_public_key: &RistrettoPublicKey,
    ) -> Result<ConfidentialOutputMaskAndValue, ConfidentialCryptoApiError> {
        let unmasked_output = unblind_output(
            output_commitment,
            output_encrypted_value,
            claim_secret,
            reciprocal_public_key,
        )?;
        Ok(unmasked_output)
    }

    pub fn try_brute_force_commitment_balances<'a, TLookup, TOutputsIter>(
        &self,
        secret_view_key: &PrivateKey,
        outputs: TOutputsIter,
        value_range: RangeInclusive<u64>,
        lookup: &mut TLookup,
    ) -> Result<Vec<Option<u64>>, ConfidentialCryptoApiError>
    where
        TLookup: ValueLookupTable,
        TOutputsIter: Iterator<Item = &'a ConfidentialOutput>,
    {
        let outputs_viewable_balance_decompressed = outputs
            .filter_map(|output| output.viewable_balance.as_ref())
            .map(ElgamalVerifiableBalance::try_from_byte_type)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| WalletCryptoError::InvalidArgument {
                name: "outputs",
                details: "Malformed viewable balance in output when decompressing ElgamalVerifiableBalance for brute \
                          forcing"
                    .to_string(),
            })?;

        let results = ElgamalVerifiableBalance::batched_brute_force(
            secret_view_key,
            value_range,
            lookup,
            &outputs_viewable_balance_decompressed,
        )
        .map_err(|e| ConfidentialCryptoApiError::ValueLookupTableError { details: e.to_string() })?;

        Ok(results)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfidentialCryptoApiError {
    #[error(transparent)]
    WalletCryptoError(#[from] WalletCryptoError),
    #[error("Confidential proof error: {0}")]
    ConfidentialProofError(#[from] ConfidentialProofError),
    #[error("Value lookup table error: {details}")]
    ValueLookupTableError { details: String },
    #[error("Negative amount")]
    NegativeAmount,
}
