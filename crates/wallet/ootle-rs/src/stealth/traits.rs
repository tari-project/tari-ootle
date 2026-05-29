//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_ootle_common_types::engine_types::crypto::OutputBody;
use tari_ootle_wallet_crypto::DecryptedData;
use tari_template_lib_types::{
    Amount,
    EncryptedData,
    crypto::PedersenCommitmentBytes,
    stealth::StealthOutputsStatement,
};

use crate::stealth::{Output, error::StealthProviderError};

pub type StealthResult<T> = Result<T, StealthProviderError>;

#[async_trait]
pub trait StealthOutputStatementFactory {
    async fn generate_outputs_statement(
        &self,
        specs: Vec<Output>,
        revealed_output_amount: Amount,
    ) -> StealthResult<(StealthOutputsStatement, RistrettoSecretKey)>;
}

#[async_trait]
pub trait InputDecryptor {
    async fn decrypt_input_data(
        &self,
        commitment: &PedersenCommitmentBytes,
        input: &OutputBody,
        skip_memo: bool,
    ) -> StealthResult<DecryptedData>;
}

/// Cryptographic operations required to claim a Layer 1 (minotari) burn.
///
/// Unlike a regular stealth transfer (which decrypts inputs with the view-only key), a burn claim
/// uses the account secret directly: the L1 burn output is a stealth output addressed to the
/// claiming account, and only the account secret can derive the key that spends the minted UTXO.
#[async_trait]
pub trait BurnClaimKeyProvider {
    /// Derive the L1 burn-claim stealth secret `s = H(p·R) + p`, where `p` is the account secret and
    /// `R` is the burn proof's `sender_offset_public_key`.
    ///
    /// This is the only key that satisfies the spend condition on the just-minted burn UTXO, so the
    /// claim transaction must be sealed with it.
    async fn derive_burn_claim_secret(
        &self,
        sender_offset_public_key: &RistrettoPublicKey,
    ) -> StealthResult<RistrettoSecretKey>;

    /// Decrypt the L1 burn output's encrypted value and mask using the account secret and the burn
    /// proof's `sender_offset_public_key`.
    async fn decrypt_burn_claim_output(
        &self,
        encrypted_data: &EncryptedData,
        commitment: &PedersenCommitmentBytes,
        sender_offset_public_key: &RistrettoPublicKey,
    ) -> StealthResult<DecryptedData>;
}

pub trait StealthSigner {
    type Signature;

    fn sign_with_stealth_key(&self, public_key: &RistrettoPublicKey) -> Result<Self::Signature, String>;
}

pub trait StealthProvider: StealthOutputStatementFactory + InputDecryptor {}
impl<T> StealthProvider for T where T: StealthOutputStatementFactory + InputDecryptor {}
