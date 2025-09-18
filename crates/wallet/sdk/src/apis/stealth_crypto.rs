//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use log::*;
use tari_crypto::{
    commitment::HomomorphicCommitmentFactory,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_engine_types::{
    crypto::{get_commitment_factory, ElgamalVerifiableBalance, PrivateOutput, ValueLookupTable},
    ConvertFromByteType,
};
use tari_ootle_common_types::{base_layer_hashing::ownership_proof_hasher64, Network};
use tari_ootle_wallet_crypto::{
    confidential,
    encrypted_data::{encrypt_value_and_mask, unblind_output},
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
    models::{ConfidentialOutputStatement, EncryptedData, ResourceAddress, StealthTransferStatement},
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes},
    types::{
        crypto::{CommitmentSignatureBytes, UtxoTag},
        Amount,
    },
};

const LOG_TARGET: &str = "tari::ootle::wallet::sdk::stealth_crypto";

#[derive(Debug, Clone, Copy)]
pub struct StealthCryptoApi;

impl StealthCryptoApi {
    pub(crate) fn new() -> Self {
        Self
    }

    pub fn derive_encrypted_data_key(
        &self,
        public_nonce: &RistrettoPublicKey,
        private_key: &RistrettoSecretKey,
    ) -> RistrettoSecretKey {
        kdfs::encrypted_data_dh_kdf_aead(private_key, public_nonce)
    }

    pub fn generate_transfer_statement<'a, A, Inputs, Outputs>(
        &self,
        inputs: Inputs,
        input_revealed_amount: A,
        output_statements: Outputs,
        output_revealed_amount: A,
    ) -> Result<StealthTransferStatement, StealthCryptoApiError>
    where
        A: Into<Amount>,
        Inputs: IntoIterator<Item = &'a UnblindedStealthInputStatement>,
        Outputs: IntoIterator<Item = &'a UnblindedStealthOutputStatement> + Clone,
    {
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
        secret: &RistrettoSecretKey,
        public_key: &RistrettoPublicKey,
        resource_address: &ResourceAddress,
    ) -> UtxoTag {
        kdfs::utxo_tag_stealth_dh(network, public_key, secret, resource_address)
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
        public_key: &RistrettoPublicKey,
        secret: &RistrettoSecretKey,
    ) -> Result<EncryptedData, StealthCryptoApiError> {
        let encryption_key = kdfs::encrypted_data_dh_kdf_aead(secret, public_key);
        let data = encrypt_value_and_mask(amount, mask, &encryption_key)?;
        Ok(data)
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

    pub fn decrypt_value_and_mask(
        &self,
        output_encrypted_value: &EncryptedData,
        output_commitment: &PedersenCommitmentBytes,
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
            .map(ElgamalVerifiableBalance::convert_from_byte_type)
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
        ownership_proof: &SchnorrSignatureBytes,
        commitment: &PedersenCommitmentBytes,
        value: u64,
        account_owner_pk: &RistrettoPublicKeyBytes,
    ) -> bool {
        // NOTE: .as_bytes() used because the tari_crypto borsh implementations serialize fixed length bytes as variable
        // length bytes of size 32
        let message = ownership_proof_hasher64(network)
            .chain(&commitment.as_bytes())
            .chain(&account_owner_pk.as_bytes())
            .finalize();

        let Ok(commitment) = PedersenCommitment::convert_from_byte_type(commitment) else {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed commitment");
            return false;
        };

        let Ok(proof_of_knowledge) = RistrettoSchnorr::convert_from_byte_type(ownership_proof) else {
            warn!(target: LOG_TARGET, "Claim burn failed - malformed proof of knowledge");
            return false;
        };

        let value_commit = get_commitment_factory().commit_value(&RistrettoSecretKey::default(), value);
        // k.G = C - v.H
        let signer_pk = commitment.as_public_key() - value_commit.as_public_key();

        if !proof_of_knowledge.verify(&signer_pk, message) {
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
