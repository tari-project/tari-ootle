//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    tari_utilities::ByteArray,
};

use super::directive::DirectiveError;

/// A `(R, s)` Schnorr signature byte-pair over a [`crate::DirectiveId`].
///
/// Stored as raw canonical byte arrays rather than `tari_crypto::RistrettoSchnorr` so that
/// borsh serialisation is straightforward and the signature type is self-contained (no borsh
/// adapters needed for the foreign newtype). Conversion to/from the in-memory Schnorr type
/// is explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DirectiveSignature {
    /// Canonical compressed Ristretto encoding of the public nonce `R`.
    public_nonce: [u8; 32],
    /// Canonical Ristretto scalar encoding of `s`.
    scalar: [u8; 32],
}

impl DirectiveSignature {
    /// Construct from the raw canonical byte pair. No validation of curve-point / scalar
    /// well-formedness is performed here; validation happens in [`DirectiveSignature::to_schnorr`]
    /// or during verification.
    pub fn new(public_nonce: [u8; 32], scalar: [u8; 32]) -> Self {
        Self { public_nonce, scalar }
    }

    pub fn public_nonce(&self) -> &[u8; 32] {
        &self.public_nonce
    }

    pub fn scalar(&self) -> &[u8; 32] {
        &self.scalar
    }

    /// Extract the byte representations from a live `RistrettoSchnorr`.
    pub fn from_schnorr(sig: &RistrettoSchnorr) -> Result<Self, DirectiveError> {
        let public_nonce = sig
            .get_public_nonce()
            .as_bytes()
            .try_into()
            .map_err(|_| DirectiveError::BadSignatureEncoding)?;
        let scalar = sig
            .get_signature()
            .as_bytes()
            .try_into()
            .map_err(|_| DirectiveError::BadSignatureEncoding)?;
        Ok(Self { public_nonce, scalar })
    }

    /// Convert back to a `RistrettoSchnorr`, validating canonical encoding in the process.
    pub fn to_schnorr(&self) -> Result<RistrettoSchnorr, DirectiveError> {
        let public_nonce = RistrettoPublicKey::from_canonical_bytes(&self.public_nonce)
            .map_err(|_| DirectiveError::BadSignatureEncoding)?;
        let scalar =
            RistrettoSecretKey::from_canonical_bytes(&self.scalar).map_err(|_| DirectiveError::BadSignatureEncoding)?;
        Ok(RistrettoSchnorr::new(public_nonce, scalar))
    }
}
