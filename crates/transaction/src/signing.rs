//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Transaction signing.
//!
//! Transaction authorization and seal signatures are standard sr25519 signatures
//! ([`Sr25519SignatureBytes`]) under [`TRANSACTION_SIGNING_CONTEXT`]. sr25519 keys are Ristretto255
//! keys, so the keys tari_crypto manages sign directly, and off-the-shelf sr25519 signers (HSMs, MPC
//! vaults, Polkadot wallets) produce signatures that verify here unchanged.

use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_ootle_common_types::signature::{SigningContext, Sr25519SignatureBytes};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

/// The sr25519 signing context of every transaction authorization and seal signature.
pub const TRANSACTION_SIGNING_CONTEXT: TransactionSigningContext =
    TransactionSigningContext(SigningContext::new(b"substrate"));

/// The message a transaction signature signs: a 64-byte digest of the transaction under the
/// transaction hash domain.
///
/// Only the transaction hashers in this crate construct one (see [`Signable`](crate::Signable)), so
/// every signature [`TRANSACTION_SIGNING_CONTEXT`] produces is over a transaction digest the signing
/// code computed itself. An external signer receives [`as_bytes`](Self::as_bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionMessage([u8; 64]);

impl TransactionMessage {
    pub(crate) const fn new(digest: [u8; 64]) -> Self {
        Self(digest)
    }

    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl AsRef<[u8]> for TransactionMessage {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// sr25519 under schnorrkel's default context, the only one many stock sr25519 signers support,
/// over [`TransactionMessage`]s.
///
/// Separation from Substrate chains comes from the message length. A standard Substrate signing payload is
/// either longer than 64 bytes, since it ends with the 32-byte genesis and block hashes after the call,
/// or hashed to 32 bytes when longer than 256. Every message here is 64 bytes, so a key signing through it
/// can never be made to sign a Substrate extrinsic.
///
/// The separation runs one way. Any other signer that signs caller-supplied bytes under `substrate` with
/// the same key can be handed a transaction message, and its signature authorizes that transaction, so a
/// transaction key is single-purpose, and a guard in front of an external signer derives the message from
/// a transaction it has inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionSigningContext(SigningContext<'static>);

impl TransactionSigningContext {
    pub const fn as_bytes(&self) -> &'static [u8] {
        self.0.as_bytes()
    }

    /// Signs a transaction `message` with `secret_key`.
    pub fn sign(&self, secret_key: &RistrettoSecretKey, message: &TransactionMessage) -> Sr25519SignatureBytes {
        self.0.sign(secret_key, message.as_bytes())
    }

    /// True when `signature` is `public_key`'s signature over `message`. See [`SigningContext::verify`].
    #[must_use]
    pub fn verify(
        &self,
        signature: &Sr25519SignatureBytes,
        public_key: &RistrettoPublicKeyBytes,
        message: &TransactionMessage,
    ) -> bool {
        self.0.verify(signature, public_key, message.as_bytes())
    }

    /// True when every item verifies. See [`SigningContext::verify_batch`].
    #[must_use]
    pub fn verify_batch(
        &self,
        items: &[(&Sr25519SignatureBytes, &RistrettoPublicKeyBytes, &TransactionMessage)],
    ) -> bool {
        self.0.verify_batch(items)
    }

    /// The challenge scalar of a signature over `message`. See [`SigningContext::challenge`].
    pub fn challenge(
        &self,
        public_nonce: &RistrettoPublicKey,
        public_key: &RistrettoPublicKey,
        message: &TransactionMessage,
    ) -> RistrettoSecretKey {
        self.0.challenge(public_nonce, public_key, message.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stock sr25519 signer's signature verifies as a transaction signature: this one is from
    /// `@scure/sr25519` 2.4.0 under its default context, over the 64-byte message `0x00..0x3f`.
    #[test]
    fn a_stock_sr25519_signature_verifies_under_the_transaction_context() {
        let message = TransactionMessage::new(std::array::from_fn(|i| i as u8));
        let public_key =
            RistrettoPublicKeyBytes::from_hex("ccddafdb8188e3edb0984780053ac7f357f1741c59292884d9fe3e5832d2f90f")
                .unwrap();
        let signature: Sr25519SignatureBytes =
            "d8280e79fe66ee85d9f20cdbfccff261bbb7401140d098a54ec1b046bdfd670a50fa8e16adbe36b86fc1ac6e4c86921eae2817b5f86fdec12424d482077abc84"
                .parse()
                .unwrap();

        assert!(TRANSACTION_SIGNING_CONTEXT.verify(&signature, &public_key, &message));
    }
}
