//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use log::*;
use tari_crypto::{
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey},
    signatures::CommitmentSignature,
};
use tari_engine_types::{
    crypto::{get_commitment_factory, ElgamalVerifiableBalance, PrivateOutput, ValueLookupTable},
    FromByteType,
};
use tari_ootle_common_types::{base_layer_hashing::ownership_proof_hasher64, Network};
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
    prelude::{crypto::CommitmentSignatureBytes, PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    types::{crypto::UtxoTagByte, Amount},
};

const LOG_TARGET: &str = "tari::ootle::wallet::sdk::stealth_crypto";

pub struct StealthCryptoApi;

impl StealthCryptoApi {
    pub(crate) fn new() -> Self {
        Self
    }

    pub fn derive_encrypted_data_key_for_receiver(
        &self,
        public_nonce: &RistrettoPublicKey,
        private_key: &RistrettoSecretKey,
    ) -> RistrettoSecretKey {
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

    pub fn derive_stealth_output_tag(
        &self,
        network: Network,
        dest_public_key: &RistrettoPublicKeyBytes,
    ) -> UtxoTagByte {
        kdfs::derive_stealth_output_tag(network, dest_public_key)
    }

    pub fn derive_stealth_owner_public_key(
        &self,
        network: Network,
        public_key: &RistrettoPublicKey,
        nonce_secret: &RistrettoSecretKey,
    ) -> RistrettoPublicKey {
        kdfs::owner_stealth_dh_stealth_address(network, public_key, nonce_secret)
    }

    pub fn derive_stealth_owner_secret(
        &self,
        network: Network,
        secret_key: &RistrettoSecretKey,
        public_nonce: &RistrettoPublicKey,
    ) -> RistrettoSecretKey {
        kdfs::owner_stealth_dh_secret(network, secret_key, public_nonce)
    }

    pub fn encrypt_value_and_mask(
        &self,
        amount: u64,
        mask: &RistrettoSecretKey,
        public_nonce: &RistrettoPublicKey,
        secret: &RistrettoSecretKey,
    ) -> Result<EncryptedData, StealthCryptoApiError> {
        let data = encrypt_value_and_mask(amount, mask, public_nonce, secret)?;
        Ok(data)
    }

    pub fn extract_value_and_mask(
        &self,
        encryption_key: &RistrettoSecretKey,
        commitment: &PedersenCommitmentBytes,
        encrypted_data: &EncryptedData,
    ) -> Result<(u64, RistrettoSecretKey), StealthCryptoApiError> {
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
        claim_secret: &RistrettoSecretKey,
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
        secret_view_key: &RistrettoSecretKey,
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

    pub fn validate_burn_claim_ownership_proof(
        &self,
        network: Network,
        ownership_proof: &CommitmentSignatureBytes,
        commitment: &PedersenCommitmentBytes,
        account_owner_pk: &RistrettoPublicKeyBytes,
    ) -> bool {
        let message = ownership_proof_hasher64(network)
            .chain(&ownership_proof.public_nonce().as_bytes())
            .chain(&commitment.as_bytes())
            .chain(&account_owner_pk)
            .finalize();

        let Ok(commitment) = PedersenCommitment::try_from_byte_type(commitment) else {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed commitment");
            return false;
        };

        let Ok(proof_of_knowledge) = CommitmentSignature::try_from_byte_type(ownership_proof) else {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed proof of knowledge");
            return false;
        };

        if !proof_of_knowledge.verify_challenge(&commitment, &message, get_commitment_factory()) {
            warn!(target: LOG_TARGET, "Claim burn failed - signature verification failed");
            return false;
        }

        true
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
