//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Claiming Layer 1 (minotari) burns on the Ootle network.
//!
//! When TARI is burned on the L1 base layer, the burn output can be claimed into an Ootle account
//! by submitting a proof. [`ClaimBurn`] builds the claim transaction: it mints the burned funds as
//! a confidential UTXO and immediately spends it into a stealth output owned by the claiming
//! account (revealing a small amount to pay the fee).
//!
//! The L1 burn output is a stealth output addressed to the claiming account, so the transaction is
//! sealed with a derived stealth claim key `s = H(p·R) + p` (not the account key). [`prepare`] returns
//! that sealer alongside the unsigned transaction.
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
//! // `claim_proof` and `encrypted_data` come from the L1 (minotari) wallet's burn output.
//! let (unsigned_tx, sealer) = ClaimBurn::new(&provider, claim_proof, encrypted_data)
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
pub use tari_ootle_common_types::engine_types::confidential::{ClaimBurnOutputData, MinotariBurnClaimProof};
use tari_ootle_common_types::engine_types::stealth::validate_transfer;
use tari_ootle_transaction::{Transaction, UnsealedTransaction, UnsignedTransaction};
use tari_ootle_wallet_crypto::{StealthCryptoApi, balance_proof::generate_stealth_balance_proof_signature, memo::Memo};
use tari_template_lib_types::{
    Amount,
    EncryptedData,
    constants::TARI_TOKEN,
    stealth::{StealthInput, StealthInputsStatement, StealthTransferStatement},
};

use crate::{
    Address,
    provider::{Provider, WalletProvider},
    signer,
    stealth::{Output, StealthProviderError},
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
    encrypted_data: EncryptedData,
    max_fee: Amount,
    recipient: Option<Address>,
    memo: Option<Memo>,
}

impl<'a, P: Provider> ClaimBurn<'a, P> {
    /// Create a new burn claim builder.
    ///
    /// `claim_proof` and `encrypted_data` are produced by the L1 (minotari) wallet for the burn
    /// output being claimed (the wallet daemon bundles them as `ClaimBurnProofContents`).
    pub fn new(provider: &'a P, claim_proof: MinotariBurnClaimProof, encrypted_data: EncryptedData) -> Self {
        Self {
            provider,
            claim_proof,
            encrypted_data,
            max_fee: Amount::zero(),
            recipient: None,
            memo: None,
        }
    }

    /// Set the maximum fee to pay for the claim. This amount is revealed from the claimed funds to
    /// pay the transaction fee; any overpayment is refunded. A positive fee is required.
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
    pub async fn prepare(self) -> WalletResult<(UnsignedTransaction, BurnClaimSealer)> {
        let Self {
            provider,
            claim_proof,
            encrypted_data,
            max_fee,
            recipient,
            memo,
        } = self;

        let network = provider.network();
        let wallet = provider.wallet();
        let claimant = provider.default_signer_address().clone();

        // `R`, the public nonce the L1 UTXO was burnt with.
        let sender_offset_public_key: RistrettoPublicKey = claim_proof
            .sender_offset_public_key
            .try_from_byte_type()
            .map_err(|e| StealthProviderError::UnexpectedError {
                details: format!("Invalid sender_offset_public_key in burn proof: {e}"),
            })?;

        // `s = H(p·R) + p`. The L1 ownership proof commits the burn to `s·G`, so this is the only key
        // that can satisfy the spend condition on the minted burn UTXO. It seals the claim transaction.
        let stealth_secret = wallet.derive_burn_claim_secret(&sender_offset_public_key).await?;
        let stealth_claim_pk = RistrettoPublicKey::from_secret_key(&stealth_secret).to_byte_type();

        if !StealthCryptoApi::new().validate_burn_claim_ownership_proof(
            network,
            &claim_proof.ownership_proof,
            &claim_proof.commitment,
            claim_proof.value,
            &stealth_claim_pk,
        ) {
            return Err(StealthProviderError::BurnClaimOwnershipProofInvalid.into());
        }

        let decrypted = wallet
            .decrypt_burn_claim_output(&encrypted_data, &claim_proof.commitment, &sender_offset_public_key)
            .await?;

        let max_fee = u64::try_from(max_fee.to_u128()).map_err(|_| StealthProviderError::UnexpectedError {
            details: "max_fee exceeds u64::MAX".to_string(),
        })?;
        if max_fee == 0 {
            return Err(StealthProviderError::UnexpectedError {
                details: "A positive max_fee is required to claim an L1 burn".to_string(),
            }
            .into());
        }

        // Reveal `max_fee` to pay the fee; the remainder becomes a stealth output owned by the claimant.
        let claimed_amount = decrypted.value();
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
        let (outputs_statement, agg_output_mask) = wallet
            .generate_outputs_statement(vec![output], Amount::from(max_fee))
            .await?;

        // The single stealth input is the burn UTXO minted by the `claim_burn` instruction. Its
        // commitment comes from the proof and its mask from decrypting the L1 output.
        let inputs_statement =
            StealthInputsStatement::new(vec![StealthInput::from(claim_proof.commitment)], Amount::zero());
        let agg_input_mask = decrypted.mask().clone();

        let balance_proof = generate_stealth_balance_proof_signature(
            &agg_input_mask,
            &agg_output_mask,
            &inputs_statement,
            &outputs_statement,
        );

        let transfer = StealthTransferStatement {
            inputs_statement,
            outputs_statement,
            balance_proof: Some(balance_proof),
            covenant_claims: Vec::new(),
        };

        // Sanity check the constructed transfer balances before paying any fees.
        if let Err(err) = validate_transfer(&transfer, None) {
            return Err(StealthProviderError::UnexpectedError {
                details: format!("Constructed burn claim transfer is invalid: {err}"),
            }
            .into());
        }

        // Re-use the L1 burn output's encrypted data when minting the UTXO. The engine accepts any
        // encrypted data here, so this is not strictly required, but it keeps the values consistent.
        let output_data = ClaimBurnOutputData { encrypted_data };

        let unsigned_tx = Transaction::builder(network)
            .with_fee_instructions_builder(|builder| {
                builder
                    // Mint the burned funds as a confidential UTXO.
                    .claim_burn(claim_proof, output_data)
                    // Spend it into the stealth output, revealing `max_fee` for the fee.
                    .stealth_transfer(TARI_TOKEN, transfer)
                    .put_last_instruction_output_on_workspace("fee")
                    .pay_fee_from_bucket("fee")
            })
            .build_unsigned();

        Ok((unsigned_tx, BurnClaimSealer::new(stealth_secret, claimant)))
    }
}

/// Seals a burn claim transaction with the derived stealth claim key `s`.
///
/// This is the only key that can satisfy the spend condition on the minted burn UTXO, so it is used
/// instead of the wallet's account key. Implements [`TransactionSealSigner`] (for
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
