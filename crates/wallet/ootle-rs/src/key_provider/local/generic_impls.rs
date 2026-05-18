//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::mem;

use async_trait::async_trait;
use ootle_byte_type::{FromByteType, ToByteType};
use signature::hazmat::PrehashSigner;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_transaction::{
    Signable,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransaction,
    UnsignedTransaction,
};
use tari_ootle_wallet_crypto::{
    OutputWitness,
    StealthCryptoApi,
    StealthOutputWitness,
    bullet_proof::generate_extended_bullet_proof,
    pay_to::PayTo,
    viewable_balance_proof::generate_elgamal_viewable_balance_proof,
};
use tari_template_lib_types::{
    Amount,
    EncryptedData,
    crypto::RistrettoPublicKeyBytes,
    stealth::{SpendCondition, StealthOutputsStatement, StealthUnspentOutput, UnspentOutput},
};
use tokio::task;

use crate::{
    Address,
    key_provider::{LocalKeyProvider, OutputMaskProvider},
    signer,
    signer::StealthKeyPrehashSigner,
    stealth::{Output, StealthOutputStatementFactory, StealthProviderError, StealthResult},
    transaction::{TransactionSigner, TransactionStealthKeySigner},
    wallet::TransactionAuthorization,
};

#[async_trait]
impl<C> TransactionSigner for LocalKeyProvider<C>
where C: PrehashSigner<(RistrettoSchnorr, RistrettoPublicKey)> + Send + Sync
{
    fn address(&self) -> &Address {
        &self.address
    }

    async fn sign_transaction(&self, message: &UnsealedTransaction) -> signer::Result<TransactionSealSignature> {
        let message = message.to_signing_message(());
        let (signature, public_key) = self.credentials.sign_prehash(&message)?;
        let sig = TransactionSealSignature::new(public_key.to_byte_type(), signature.to_byte_type());
        Ok(sig)
    }

    async fn sign_authorization(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization> {
        let message = tx.to_signing_message(seal_signer);
        let (signature, public_key) = self.credentials.sign_prehash(&message)?;
        let sig = TransactionSignature::new(public_key.to_byte_type(), signature.to_byte_type());
        Ok(sig.into())
    }
}

#[async_trait]
impl<C: OutputMaskProvider + Send + Sync> StealthOutputStatementFactory for LocalKeyProvider<C> {
    async fn generate_outputs_statement(
        &self,
        specs: Vec<Output>,
        revealed_output_amount: Amount,
    ) -> StealthResult<(StealthOutputsStatement, RistrettoSecretKey)> {
        let mut outputs = Vec::with_capacity(specs.len());
        let mut witnesses = Vec::with_capacity(specs.len());
        let mut agg_output_mask = RistrettoSecretKey::default();
        for spec in specs {
            let StealthOutputWitness {
                mut witness,
                spend_condition,
                tag,
            } = create_output_witness(&self.credentials, spec).await?;

            let commitment = witness.to_commitment();
            agg_output_mask = &agg_output_mask + &witness.mask;

            outputs.push(StealthUnspentOutput {
                output: UnspentOutput {
                    commitment: commitment.to_byte_type(),
                    sender_public_nonce: witness.sender_public_nonce.to_byte_type(),
                    minimum_value_promise: witness.minimum_value_promise,
                    viewable_balance_proof: witness
                        .resource_view_key
                        .as_ref()
                        .map(|pk| {
                            generate_elgamal_viewable_balance_proof(&witness.mask, witness.amount, &commitment, pk)
                        })
                        .transpose()?,
                    // Move the encrypted data out of the witness, we don't need it in the bullet proof generation
                    encrypted_data: mem::replace(&mut witness.encrypted_data, EncryptedData::empty()),
                },
                spend_condition,
                tag,
            });

            witnesses.push(witness);
        }

        let agg_range_proof = task::spawn_blocking(move || generate_extended_bullet_proof(&witnesses))
            .await
            .map_err(|e| StealthProviderError::SpawnBlockingPanic { details: e.to_string() })?
            .map_err(|e| StealthProviderError::RangeProofError { details: e.to_string() })?;

        Ok((
            StealthOutputsStatement {
                outputs,
                revealed_output_amount,
                agg_range_proof,
            },
            agg_output_mask,
        ))
    }
}

async fn create_output_witness<K: OutputMaskProvider>(
    key_provider: &K,
    spec: Output,
) -> Result<StealthOutputWitness, StealthProviderError> {
    let mask = key_provider
        .next_mask()
        .await
        .map_err(|e| StealthProviderError::UnexpectedError { details: e.to_string() })?;
    let Output {
        destination,
        amount,
        resource_address,
        resource_view_key,
        memo,
        pay_to,
        ..
    } = spec;

    let destination: RistrettoOotleAddress =
        destination
            .try_from_byte_type()
            .map_err(|_| StealthProviderError::InvalidDestinationAddress {
                details: format!("{destination} is not a valid RistrettoOotleAddress"),
            })?;

    let crypto_api = StealthCryptoApi::new();

    let (nonce_secret, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let encrypted_data = crypto_api.encrypt_value_and_mask(
        amount.get(),
        &mask,
        destination.view_only_key(),
        &nonce_secret,
        memo.as_ref(),
    )?;

    let spend_condition = match pay_to {
        PayTo::StealthPublicKey => {
            // Create stealth address that the destination can use at spend time
            let output_owner_public_key = crypto_api.derive_stealth_owner_public_key(
                destination.network(),
                destination.account_key(),
                &nonce_secret,
            );

            SpendCondition::Signed(output_owner_public_key.to_byte_type())
        },
        PayTo::AccessRule(access_rule) => SpendCondition::AccessRule(access_rule),
    };

    let witness = OutputWitness {
        amount: amount.get(),
        mask,
        sender_public_nonce: public_nonce,
        encrypted_data,
        minimum_value_promise: spec.minimum_value_promise,
        resource_view_key,
    };

    let derived_tag = spec.utxo_tag.unwrap_or_else(|| {
        crypto_api.derive_stealth_output_tag(
            destination.network(),
            &nonce_secret,
            destination.view_only_key(),
            &resource_address,
        )
    });

    Ok(StealthOutputWitness {
        witness,
        spend_condition,
        tag: derived_tag,
    })
}

#[async_trait]
impl<C: StealthKeyPrehashSigner<(RistrettoSchnorr, RistrettoPublicKey)> + Send + Sync> TransactionStealthKeySigner
    for LocalKeyProvider<C>
{
    async fn sign_authorization_with_stealth(
        &self,
        public_nonce: &RistrettoPublicKey,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization> {
        let (sig, pk) = self
            .credentials
            .sign_prehash_with_stealth_key(public_nonce, &tx.to_signing_message(seal_signer))
            .await?;
        let sig = TransactionSignature::new(pk.to_byte_type(), sig.to_byte_type());
        Ok(sig.into())
    }

    async fn seal_transaction_with_stealth(
        &self,
        public_nonce: &RistrettoPublicKey,
        message: &UnsealedTransaction,
    ) -> signer::Result<TransactionSealSignature> {
        let message = message.to_signing_message(());
        let (signature, public_key) = self
            .credentials
            .sign_prehash_with_stealth_key(public_nonce, &message)
            .await?;
        let sig = TransactionSealSignature::new(public_key.to_byte_type(), signature.to_byte_type());
        Ok(sig)
    }
}
