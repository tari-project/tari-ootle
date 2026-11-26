//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Claiming Layer 1 (minotari) burns on the Ootle network.
//!
//! When TARI is burned on the L1 base layer, the burn output can be claimed into an Ootle account
//! by submitting a proof. [`ClaimBurn`] builds the claim transaction: the claim reveals the burned
//! funds in a bucket, which the same transaction pays into a stealth output owned by the claiming
//! account (revealing a small amount to pay the fee).
//!
//! The L1 burn output names a stealth claim key derived for the claiming account, so the transaction
//! is sealed with the derived stealth claim key `s = H(p·R) + p` (not the account key). [`prepare`]
//! returns that sealer alongside the unsigned transaction.
//!
//! [`prepare`]: ClaimBurn::prepare
//!
//! ```rust,ignore
//! use ootle_rs::{
//!     claim_burn::ClaimBurn,
//!     provider::{Provider, WalletProvider},
//!     TransactionRequest,
//! };
//!
//! // `claim_proof` comes from the L1 (minotari) wallet's burn output.
//! let (unsigned_tx, sealer) = ClaimBurn::new(&provider, claim_proof, max_epoch)
//!     .with_max_fee(1000u64)
//!     .prepare()
//!     .await?;
//!
//! let tx = TransactionRequest::default()
//!     .with_transaction(unsigned_tx)
//!     .build(&sealer)
//!     .await?;
//!
//! provider.send_transaction(tx).await?.watch().await?;
//! ```

use std::num::NonZeroU64;

use async_trait::async_trait;
use ootle_byte_type::{FromByteType, ToByteType};
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
// Re-export the burn proof types so callers don't need to reach into `tari_ootle_common_types`.
pub use tari_ootle_common_types::engine_types::confidential::MinotariBurnClaimProof;
use tari_ootle_common_types::engine_types::stealth::validate_transfer;
use tari_ootle_transaction::{Epoch, Transaction, UnsealedTransaction, UnsignedTransaction};
use tari_ootle_wallet_crypto::{StealthCryptoApi, memo::Memo};
use tari_template_lib_types::{Amount, constants::TARI_TOKEN, stealth::RevealedOutput};

use crate::{
    Address,
    provider::{Provider, WalletProvider},
    signer,
    stealth::{Output, ResolvedStealthTransferSpec, StealthProviderError},
    transaction::TransactionSealSigner,
    wallet::{NetworkWallet, OotleWallet, WalletResult},
};

/// Default memo attached to the claimed funds output.
const DEFAULT_CLAIM_MEMO: &str = "Burnt funds claimed from L1";

/// Builder for claiming a Layer 1 (minotari) burn into an Ootle account.
///
/// See the [module documentation](crate::claim_burn) for the full flow.
pub struct ClaimBurn<'a, P> {
    provider: &'a P,
    claim_proof: MinotariBurnClaimProof,
    max_fee: Amount,
    max_epoch: Epoch,
    recipient: Option<Address>,
    memo: Option<Memo>,
}

impl<'a, P: Provider> ClaimBurn<'a, P> {
    /// Create a new burn claim builder.
    ///
    /// `claim_proof` is produced by the L1 (minotari) wallet for the burn output being claimed.
    /// `max_epoch` is the last epoch the claim transaction may be sequenced in. It is required:
    /// every transaction carries a bounded validity window.
    pub fn new(provider: &'a P, claim_proof: MinotariBurnClaimProof, max_epoch: Epoch) -> Self {
        Self {
            provider,
            claim_proof,
            max_fee: Amount::zero(),
            max_epoch,
            recipient: None,
            memo: None,
        }
    }

    /// Set the fee to pay for the claim. This amount is revealed from the claimed funds and paid
    /// from a bucket, which the engine does not refund: the whole amount is spent, even where the
    /// claim costs less. A positive fee is required.
    ///
    /// A dry run's fee is the figure the indexer that ran it reports. Bound it by what the claim
    /// should cost before passing it here.
    pub fn with_max_fee<A: Into<Amount>>(mut self, max_fee: A) -> Self {
        self.max_fee = max_fee.into();
        self
    }

    /// Claim the funds to a different address than the claiming account. Defaults to the claiming
    /// account (the provider's default signer).
    pub fn to_recipient(mut self, recipient: Address) -> Self {
        self.recipient = Some(recipient);
        self
    }

    /// Attach a custom (encrypted) memo to the claimed funds output. Defaults to a "Burnt funds
    /// claimed from L1" message.
    pub fn with_memo(mut self, memo: Memo) -> Self {
        self.memo = Some(memo);
        self
    }

    /// Convenience method to attach a text memo to the claimed funds output.
    ///
    /// # Panics
    /// Panics if the message is too long to fit in a memo.
    pub fn with_memo_message<T: Into<Box<str>>>(self, message: T) -> Self {
        self.with_memo(Memo::new_message(message).expect("Memo message too long"))
    }
}

impl<'a, P: WalletProvider<Wallet = OotleWallet>> ClaimBurn<'a, P> {
    /// Build the unsigned claim transaction and the sealer that signs it with the derived stealth
    /// claim key.
    ///
    /// Submit it via a [`TransactionRequest`](crate::TransactionRequest) sealed with the returned
    /// [`BurnClaimSealer`], or estimate fees first with
    /// [`sign_and_send_dry_run_with`](crate::provider::IndexerProvider::sign_and_send_dry_run_with).
    /// The fee set with [`with_max_fee`](Self::with_max_fee) is spent in full, so bound a dry run's
    /// figure before building the claim at it.
    pub async fn prepare(self) -> WalletResult<(UnsignedTransaction, BurnClaimSealer)> {
        let Self {
            provider,
            claim_proof,
            max_fee,
            max_epoch,
            recipient,
            memo,
        } = self;

        let network = provider.network();
        let wallet = provider.wallet();
        let claimant = provider.default_signer_address().clone();

        // `R`, the public nonce the L1 UTXO was burnt with.
        let sender_offset_public_key: RistrettoPublicKey = claim_proof
            .output
            .sender_offset_public_key
            .try_from_byte_type()
            .map_err(|e| StealthProviderError::UnexpectedError {
                details: format!("Invalid sender_offset_public_key in burn proof: {e}"),
            })?;

        // `s = H(p·R) + p`. The burn output names `S = s·G` as its claim key, and the engine only accepts a claim
        // that `S` signs, so the claim key seals the claim transaction.
        let stealth_secret = wallet.derive_burn_claim_secret(&sender_offset_public_key).await?;
        let stealth_claim_pk = RistrettoPublicKey::from_secret_key(&stealth_secret).to_byte_type();
        let claim_public_key = claim_proof.output.features.claim_public_key;
        if stealth_claim_pk != claim_public_key {
            return Err(StealthProviderError::BurnClaimNotForThisWallet {
                claim_public_key,
                wallet_claim_public_key: stealth_claim_pk,
            }
            .into());
        }

        if !StealthCryptoApi::new().validate_burn_claim_ownership_proof(
            network,
            &claim_proof.ownership_proof,
            &claim_proof.commitment,
            claim_proof.value,
            &stealth_claim_pk,
        ) {
            return Err(StealthProviderError::BurnClaimOwnershipProofInvalid.into());
        }

        let max_fee = u64::try_from(max_fee.to_u128()).map_err(|_| StealthProviderError::UnexpectedError {
            details: "max_fee exceeds u64::MAX".to_string(),
        })?;
        if max_fee == 0 {
            return Err(StealthProviderError::UnexpectedError {
                details: "A positive max_fee is required to claim an L1 burn".to_string(),
            }
            .into());
        }

        // Reveal `max_fee` to pay the fee; the remainder becomes a stealth output owned by the claimant. The ownership
        // proof checked above binds `claim_proof.value` to the burn commitment.
        let claimed_amount = claim_proof.value;
        let final_amount = claimed_amount.checked_sub(max_fee).filter(|amount| *amount > 0).ok_or(
            StealthProviderError::BurnClaimFeeTooHigh {
                claimed: claimed_amount,
                max_fee,
            },
        )?;
        let final_amount = NonZeroU64::new(final_amount).expect("final_amount checked to be positive above");

        // The claimed-funds output is an ordinary stealth output to the recipient (the claimant by default).
        let recipient = recipient.unwrap_or_else(|| claimant.clone());
        let memo = memo.unwrap_or_else(|| Memo::new_message(DEFAULT_CLAIM_MEMO).expect("valid memo"));
        let output = Output::new(recipient, TARI_TOKEN, final_amount).with_memo(memo);
        let transfer = wallet
            .create_transfer_statement(ResolvedStealthTransferSpec {
                inputs: vec![],
                revealed_input_amount: Amount::from(claimed_amount),
                outputs: vec![output],
                // The claim key seals this transaction, so it is the only badge in scope to take the revealed fee.
                revealed_output: Some(RevealedOutput::new(Amount::from(max_fee), stealth_claim_pk)),
            })
            .await?;

        // Sanity check the constructed transfer balances before paying any fees.
        if let Err(err) = validate_transfer(&transfer, None) {
            return Err(StealthProviderError::UnexpectedError {
                details: format!("Constructed burn claim transfer is invalid: {err}"),
            }
            .into());
        }

        let unsigned_tx = Transaction::builder(network, max_epoch)
            .with_fee_instructions_builder(|builder| {
                builder
                    // Reveal the burned funds in a bucket.
                    .claim_burn(claim_proof)
                    .put_last_instruction_output_on_workspace("claimed")
                    // Pay the bucket into the stealth output, revealing `max_fee` for the fee.
                    .stealth_transfer_with_input_bucket(TARI_TOKEN, transfer, "claimed")
                    .put_last_instruction_output_on_workspace("fee")
                    .pay_fee_from_bucket("fee")
            })
            .build_unsigned();

        Ok((unsigned_tx, BurnClaimSealer::new(stealth_secret, claimant)))
    }
}

/// Seals a burn claim transaction with the derived stealth claim key `s`.
///
/// The engine accepts a claim only when the claim key the burn output names signs it, so this key is
/// used instead of the wallet's account key. Implements [`TransactionSealSigner`] (for
/// [`TransactionRequest::build`](crate::TransactionRequest::build)) and [`NetworkWallet`] (for dry-run
/// fee estimation). A burn claim needs no additional authorizations — the seal authorizes the spend.
#[derive(Clone)]
pub struct BurnClaimSealer {
    secret: RistrettoSecretKey,
    address: Address,
}

impl BurnClaimSealer {
    pub(crate) fn new(secret: RistrettoSecretKey, address: Address) -> Self {
        Self { secret, address }
    }
}

impl std::fmt::Debug for BurnClaimSealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BurnClaimSealer")
            .field("secret", &"<redacted>")
            .field("address", &self.address)
            .finish()
    }
}

#[async_trait]
impl TransactionSealSigner for BurnClaimSealer {
    async fn seal_transaction(&self, transaction: UnsealedTransaction) -> signer::Result<Transaction> {
        Ok(transaction.seal(&self.secret))
    }
}

impl NetworkWallet for BurnClaimSealer {
    fn default_address(&self) -> &Address {
        &self.address
    }

    async fn sign_transaction(&self, unsigned: UnsignedTransaction) -> WalletResult<Transaction> {
        // A burn claim carries no additional authorizations, so seal directly with the claim key.
        Ok(unsigned.finish().seal(&self.secret))
    }
}
