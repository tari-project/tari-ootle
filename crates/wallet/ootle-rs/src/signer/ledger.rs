//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Ledger hardware-wallet signer.
//!
//! [`LedgerSigner`] implements [`TransactionSigner`] and [`TransactionSealSigner`] by streaming the
//! canonical signing preimage to an Ootle Ledger device, which recomputes the signing message,
//! displays a summary for the user to approve, and returns a Ristretto Schnorr signature. The two
//! procedures map to the device's two signing modes:
//!   - [`TransactionSigner::sign_authorization`] → [`SignMode::AddSigner`]
//!   - [`TransactionSigner::sign_transaction`] / [`TransactionSealSigner::seal_transaction`] → [`SignMode::Seal`]
//!
//! Transactions are signed with the account key branch ([`KeyType::Account`]), matching the
//! address returned by [`LedgerSigner::address`].

use async_trait::async_trait;
use ootle_ledger_common::arg_types::{KeyType, SignMode, SigningField};
use tari_ledger_client::{Exchange, LedgerClient, LedgerClientError};
use tari_ootle_address::OotleAddress;
use tari_ootle_transaction::{
    IntoSigned,
    PreimageField,
    PreimageSegment,
    Transaction,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransaction,
    UnsignedTransaction,
};
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{
    Address,
    Network,
    signer::{self, SignerError},
    transaction::{TransactionSealSigner, TransactionSigner},
    wallet::TransactionAuthorization,
};

/// A signer backed by an Ootle Ledger device over an arbitrary APDU transport `T`.
pub struct LedgerSigner<T> {
    client: LedgerClient<T>,
    address: Address,
    account: u64,
    index: u64,
}

impl<T> LedgerSigner<T>
where
    T: Exchange,
    T::Error: core::fmt::Display,
{
    /// Connect to a device-backed signer: fetch the account and view-only public keys for
    /// `(account, index)` and derive the wallet [`Address`].
    pub async fn connect(client: LedgerClient<T>, network: Network, account: u64, index: u64) -> signer::Result<Self> {
        let account_pk = client
            .get_public_key(account, index, KeyType::Account)
            .await
            .map_err(map_client_err)?;
        let view_only_pk = client
            .get_public_key(account, index, KeyType::ViewOnlyKey)
            .await
            .map_err(map_client_err)?;
        let address = OotleAddress::new(network, view_only_pk, account_pk);
        Ok(Self {
            client,
            address,
            account,
            index,
        })
    }

    /// Construct directly from an already-known address (skips the device round-trips). The caller
    /// is responsible for ensuring `address` corresponds to `(account, index)` on the device.
    pub fn with_address(client: LedgerClient<T>, address: Address, account: u64, index: u64) -> Self {
        Self {
            client,
            address,
            account,
            index,
        }
    }

    /// Stream a preimage to the device and parse the returned public key + signature.
    async fn stream(
        &self,
        mode: SignMode,
        segments: Vec<PreimageSegment>,
    ) -> signer::Result<(RistrettoPublicKeyBytes, SchnorrSignatureBytes)> {
        let refs: Vec<(SigningField, &[u8])> = segments
            .iter()
            .map(|seg| (to_wire(seg.field), seg.bytes.as_slice()))
            .collect();

        let response = self
            .client
            .sign_transaction(self.account, self.index, KeyType::Account, mode, &refs)
            .await
            .map_err(map_client_err)?;

        let public_key = RistrettoPublicKeyBytes::from(response.public_key);
        let signature = SchnorrSignatureBytes::try_from(&response.signature[..])
            .map_err(|_| SignerError::other("device returned malformed signature bytes"))?;
        Ok((public_key, signature))
    }
}

#[async_trait]
impl<T> TransactionSigner for LedgerSigner<T>
where
    T: Exchange + Send + Sync,
    T::Error: core::fmt::Display + Send + Sync,
{
    fn address(&self) -> &Address {
        &self.address
    }

    async fn sign_transaction(&self, message: &UnsealedTransaction) -> signer::Result<TransactionSealSignature> {
        let UnsealedTransaction::V1(unsealed) = message;
        let segments = TransactionSealSignature::signing_preimage_v1(unsealed);
        let (public_key, signature) = self.stream(SignMode::Seal, segments).await?;
        Ok(TransactionSealSignature::new(public_key, signature))
    }

    async fn sign_authorization(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization> {
        let UnsignedTransaction::V1(unsigned) = tx;
        let segments = TransactionSignature::signing_preimage_v1(seal_signer, unsigned);
        let (public_key, signature) = self.stream(SignMode::AddSigner, segments).await?;
        Ok(TransactionSignature::new(public_key, signature).into())
    }
}

#[async_trait]
impl<T> TransactionSealSigner for LedgerSigner<T>
where
    T: Exchange + Send + Sync,
    T::Error: core::fmt::Display + Send + Sync,
{
    async fn seal_transaction(&self, tx: UnsealedTransaction) -> signer::Result<Transaction> {
        let signature = self.sign_transaction(&tx).await?;
        Ok(<UnsealedTransaction as IntoSigned<()>>::into_signed(tx, signature))
    }
}

fn map_client_err<E: core::fmt::Display>(err: LedgerClientError<E>) -> SignerError {
    SignerError::other(err.to_string())
}

/// Map a transaction-crate preimage field to its on-the-wire protocol tag. The numeric equivalence
/// is asserted by `preimage_field_tags_match_protocol` in the ledger client tests.
fn to_wire(field: PreimageField) -> SigningField {
    match field {
        PreimageField::SchemaVersion => SigningField::SchemaVersion,
        PreimageField::SealSigner => SigningField::SealSigner,
        PreimageField::Network => SigningField::Network,
        PreimageField::FeeInstructions => SigningField::FeeInstructions,
        PreimageField::Instructions => SigningField::Instructions,
        PreimageField::Inputs => SigningField::Inputs,
        PreimageField::MinEpoch => SigningField::MinEpoch,
        PreimageField::MaxEpoch => SigningField::MaxEpoch,
        PreimageField::IsSealSignerAuthorized => SigningField::IsSealSignerAuthorized,
        PreimageField::DryRun => SigningField::DryRun,
        PreimageField::BlobHashes => SigningField::BlobHashes,
        PreimageField::Signatures => SigningField::Signatures,
    }
}
