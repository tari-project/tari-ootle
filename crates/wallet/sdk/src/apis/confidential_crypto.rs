//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::PrivateKey;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_wallet_crypto::{
    confidential,
    encrypted_data::{decrypt_data, encrypt_data, unblind_output},
    kdfs,
    memo::Memo,
    ConfidentialProofError,
    DecryptedData,
    MaskAndValue,
    OutputWitness,
    WalletCryptoError,
};
use tari_template_lib::{
    models::{ConfidentialOutputStatement, ConfidentialWithdrawProof},
    prelude::PedersenCommitmentBytes,
    types::{Amount, EncryptedData},
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
        inputs: &[MaskAndValue],
        input_revealed_amount: A,
        output_statement: Option<&OutputWitness>,
        output_revealed_amount: A,
        change_statement: Option<&OutputWitness>,
        change_revealed_amount: A,
    ) -> Result<ConfidentialWithdrawProof, ConfidentialCryptoApiError> {
        let proof = confidential::create_withdraw_proof(
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
        memo: Option<&Memo>,
    ) -> Result<EncryptedData, ConfidentialCryptoApiError> {
        let key = kdfs::encrypted_data_dh_kdf_aead(secret, public_nonce);
        let data = encrypt_data(amount, mask, &key, memo)?;
        Ok(data)
    }

    pub fn decrypt_output_data(
        &self,
        encryption_key: &PrivateKey,
        commitment: &PedersenCommitmentBytes,
        encrypted_data: &EncryptedData,
        skip_memo: bool,
    ) -> Result<DecryptedData, ConfidentialCryptoApiError> {
        let decrypted = decrypt_data(encryption_key, commitment, encrypted_data, skip_memo)?;
        Ok(decrypted)
    }

    pub fn generate_output_proof<A: Into<Amount>>(
        &self,
        statement: &OutputWitness,
        revealed_amount: A,
    ) -> Result<ConfidentialOutputStatement, ConfidentialCryptoApiError> {
        let proof = confidential::create_output_statement(
            Some(statement).filter(|s| s.amount > 0),
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
        skip_memo: bool,
    ) -> Result<DecryptedData, ConfidentialCryptoApiError> {
        let decrypted = unblind_output(
            output_commitment,
            output_encrypted_value,
            claim_secret,
            reciprocal_public_key,
            skip_memo,
        )?;
        Ok(decrypted)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfidentialCryptoApiError {
    #[error(transparent)]
    WalletCryptoError(#[from] WalletCryptoError),
    #[error("Confidential proof error: {0}")]
    ConfidentialProofError(#[from] ConfidentialProofError),
    #[error("Negative amount")]
    NegativeAmount,
}
