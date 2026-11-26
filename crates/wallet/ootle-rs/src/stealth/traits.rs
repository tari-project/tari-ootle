//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_ootle_common_types::engine_types::crypto::OutputBody;
use tari_ootle_wallet_crypto::DecryptedData;
use tari_template_lib_types::{
    crypto::PedersenCommitmentBytes,
    stealth::{RevealedOutput, StealthOutputsStatement, StealthTransferStatement},
};

use crate::stealth::{Output, ResolvedStealthTransferSpec, error::StealthProviderError};

pub type StealthResult<T> = Result<T, StealthProviderError>;

/// Produces a complete [`StealthTransferStatement`] from a resolved transfer spec.
///
/// This is the custody boundary for stealth transfers. A statement's balance proof is a Schnorr
/// signature whose private key is `Σoutput_mask - Σinput_mask`, so whichever component builds it must
/// hold every mask in the transfer. Confining that to the implementor is what lets a key store that
/// is not in-process — a wallet daemon behind an RPC, or a hardware wallet — implement this trait:
/// only the finished statement crosses the boundary, never a mask or any other secret.
///
/// The balance proof commits to the inputs and outputs statements alone, not to the transaction that
/// carries them, so a statement is self-contained. An implementor needs to know nothing about the
/// transaction being built, and callers are free to embed the returned statement in any transaction.
#[async_trait]
pub trait StealthStatementProvider {
    /// Build the inputs statement, outputs statement, aggregate range proof and balance proof for
    /// `spec`, returning them as one statement.
    ///
    /// The spec's inputs must already be resolved against the network — see
    /// [`ResolvedStealthInput`](crate::stealth::ResolvedStealthInput). Implementors recover each
    /// input's mask from its output body, and must return
    /// [`StealthProviderError::UnbalancedTransfer`] if the inputs and outputs do not balance.
    async fn create_transfer_statement(
        &self,
        spec: ResolvedStealthTransferSpec,
    ) -> StealthResult<StealthTransferStatement>;
}

/// Creates stealth outputs and the aggregate range proof over them.
///
/// Internal to local (in-process) custody: `generate_outputs_statement` returns the aggregate output
/// mask, so it must never be reachable across a process boundary. [`StealthStatementProvider`] is the
/// boundary-safe seam built on top of this.
#[async_trait]
pub(crate) trait StealthOutputStatementFactory {
    /// Create an output per spec and return the outputs statement alongside `Σoutput_mask`, which the
    /// caller needs to sign the transfer's balance proof.
    async fn generate_outputs_statement(
        &self,
        specs: Vec<Output>,
        revealed_output: Option<RevealedOutput>,
    ) -> StealthResult<(StealthOutputsStatement, RistrettoSecretKey)>;
}

/// Recovers the value and mask committed to by a stealth input.
///
/// Internal to local (in-process) custody: [`DecryptedData`] carries the input's mask, so this must
/// never be reachable across a process boundary. [`StealthStatementProvider`] is the boundary-safe
/// seam built on top of this.
#[async_trait]
pub(crate) trait InputDecryptor {
    async fn decrypt_input_data(
        &self,
        commitment: &PedersenCommitmentBytes,
        input: &OutputBody,
        skip_memo: bool,
    ) -> StealthResult<DecryptedData>;
}

/// The key required to claim a Layer 1 (minotari) burn.
///
/// The L1 burn output names a stealth claim key derived from the claiming account's secret, and the
/// engine accepts a claim only when that key signs it.
///
/// A burn claim is inherently local-custody: [`derive_burn_claim_secret`](Self::derive_burn_claim_secret)
/// hands out the key that seals the claim transaction, so unlike [`StealthStatementProvider`] this
/// trait cannot be satisfied by a remote key store as it stands.
#[async_trait]
pub trait BurnClaimKeyProvider {
    /// Derive the L1 burn-claim stealth secret `s = H(p·R) + p`, where `p` is the account secret and
    /// `R` is the burn proof's `sender_offset_public_key`.
    ///
    /// `S = s·G` is the claim key the burn output names, so the claim transaction must be signed with it.
    async fn derive_burn_claim_secret(
        &self,
        sender_offset_public_key: &RistrettoPublicKey,
    ) -> StealthResult<RistrettoSecretKey>;
}

pub trait StealthSigner {
    type Signature;

    fn sign_with_stealth_key(&self, public_key: &RistrettoPublicKey) -> Result<Self::Signature, String>;
}
