//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use tari_common_types::types::PrivateKey;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::{
    crypto::{ElgamalVerifiableBalance, PrivateOutput, ValueLookupTable},
    FromByteType,
};
use tari_ootle_wallet_crypto::{
    confidential,
    encrypted_data::{encrypt_value_and_mask, extract_value_and_mask, unblind_output},
    kdfs,
    stealth,
    ConfidentialProofError,
    MaskAndValue,
    UnblindedOutputStatement,
    UnblindedStealthInputStatement,
    UnblindedStealthOutputStatement,
    WalletCryptoError,
};
use tari_template_lib::{
    models::{ConfidentialOutputStatement, EncryptedData, StealthTransferStatement},
    prelude::PedersenCommitmentBytes,
    types::Amount,
};

pub struct StealthCryptoApi;

impl StealthCryptoApi {
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

    pub fn generate_transfer_statement<A: Into<Amount>>(
        &self,
        inputs: &[UnblindedStealthInputStatement],
        input_revealed_amount: A,
        output_statements: &[UnblindedStealthOutputStatement],
        output_revealed_amount: A,
    ) -> Result<StealthTransferStatement, StealthCryptoApiError> {
        let stmt = stealth::create_transfer_statement(
            inputs,
            input_revealed_amount
                .into()
                .non_negative_checked()
                .ok_or(StealthCryptoApiError::NegativeAmount)?,
            output_statements,
            output_revealed_amount
                .into()
                .non_negative_checked()
                .ok_or(StealthCryptoApiError::NegativeAmount)?,
        )?;
        Ok(stmt)
    }

    pub fn encrypt_value_and_mask(
        &self,
        amount: u64,
        mask: &PrivateKey,
        public_nonce: &RistrettoPublicKey,
        secret: &PrivateKey,
    ) -> Result<EncryptedData, StealthCryptoApiError> {
        let data = encrypt_value_and_mask(amount, mask, public_nonce, secret)?;
        Ok(data)
    }

    pub fn extract_value_and_mask(
        &self,
        encryption_key: &PrivateKey,
        commitment: &PedersenCommitmentBytes,
        encrypted_data: &EncryptedData,
    ) -> Result<(u64, PrivateKey), StealthCryptoApiError> {
        let value_and_mask = extract_value_and_mask(encryption_key, commitment, encrypted_data)?;
        Ok(value_and_mask)
    }

    pub fn generate_output_proof<A: Into<Amount>>(
        &self,
        statement: &UnblindedOutputStatement,
        revealed_amount: A,
    ) -> Result<ConfidentialOutputStatement, StealthCryptoApiError> {
        let proof = confidential::create_output_statement(
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
    ) -> Result<MaskAndValue, StealthCryptoApiError> {
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
    ) -> Result<Vec<Option<u64>>, StealthCryptoApiError>
    where
        TLookup: ValueLookupTable,
        TOutputsIter: Iterator<Item = &'a PrivateOutput>,
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
        .map_err(|e| StealthCryptoApiError::ValueLookupTableError { details: e.to_string() })?;

        Ok(results)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StealthCryptoApiError {
    #[error(transparent)]
    WalletCryptoError(#[from] WalletCryptoError),
    #[error("Confidential proof error: {0}")]
    ConfidentialProofError(#[from] ConfidentialProofError),
    #[error("Value lookup table error: {details}")]
    ValueLookupTableError { details: String },
    #[error("Negative amount")]
    NegativeAmount,
}
