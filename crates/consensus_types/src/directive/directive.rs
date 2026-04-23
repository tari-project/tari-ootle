//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use blake2::{Blake2b, Digest, digest::consts};
use borsh::{BorshDeserialize, BorshSerialize};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    signatures::SchnorrSignatureError,
};

use super::{body::DirectiveBody, signature::DirectiveSignature};

/// Domain tag mixed into the `DirectiveId` hash to prevent cross-protocol signature replay.
const DIRECTIVE_ID_DOMAIN: &[u8] = b"tari.ootle.consensus_directive.id.v1";

/// The ID of a [`ConsensusDirective`] — the Blake2b-256 hash of a domain-tagged canonical
/// serialisation of its [`DirectiveBody`].
///
/// Validators use the ID as the key for idempotent application: a directive whose ID already
/// appears in the applied-directives table is treated as a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DirectiveId([u8; 32]);

impl DirectiveId {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for DirectiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

/// A governance-signed directive.
///
/// Construct via [`ConsensusDirective::sign`] with the governance secret key, then transport
/// (as a serialised blob) to each validator. Validators verify with [`ConsensusDirective::verify`]
/// using the configured governance public key.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ConsensusDirective {
    body: DirectiveBody,
    signature: DirectiveSignature,
}

impl ConsensusDirective {
    /// Sign `body` with the governance secret key. Uses `tari_crypto`'s `RistrettoSchnorr::sign`
    /// which hashes the provided message into canonical form internally.
    pub fn sign<R: RngCore + CryptoRng>(
        body: DirectiveBody,
        gov_secret: &RistrettoSecretKey,
        rng: &mut R,
    ) -> Result<Self, DirectiveError> {
        let id = Self::compute_id(&body)?;
        let schnorr = RistrettoSchnorr::sign(gov_secret, id.as_bytes(), rng).map_err(DirectiveError::Sign)?;
        let signature = DirectiveSignature::from_schnorr(&schnorr)?;
        Ok(Self { body, signature })
    }

    /// Construct a directive from a pre-signed body. Callers are responsible for ensuring the
    /// signature matches; [`ConsensusDirective::verify`] performs the cryptographic check.
    pub fn from_parts(body: DirectiveBody, signature: DirectiveSignature) -> Self {
        Self { body, signature }
    }

    pub fn body(&self) -> &DirectiveBody {
        &self.body
    }

    pub fn signature(&self) -> &DirectiveSignature {
        &self.signature
    }

    /// Returns the ID of this directive.
    pub fn id(&self) -> DirectiveId {
        Self::compute_id(&self.body).expect("body borsh-serialisation is infallible")
    }

    fn compute_id(body: &DirectiveBody) -> Result<DirectiveId, DirectiveError> {
        let body_bytes = borsh::to_vec(body).map_err(|e| DirectiveError::Serialise(e.to_string()))?;
        let hash = Blake2b::<consts::U32>::new()
            .chain_update(DIRECTIVE_ID_DOMAIN)
            .chain_update(body_bytes)
            .finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&hash);
        Ok(DirectiveId(out))
    }

    /// Verify the signature against `gov_pk`. Returns `Ok(())` on success; otherwise a specific
    /// [`DirectiveError`] variant describing the failure.
    pub fn verify(&self, gov_pk: &RistrettoPublicKey) -> Result<(), DirectiveError> {
        let id = Self::compute_id(&self.body)?;
        let schnorr = self.signature.to_schnorr()?;
        if !schnorr.verify(gov_pk, id.as_bytes()) {
            return Err(DirectiveError::InvalidSignature);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DirectiveError {
    #[error("Signature is not valid for this directive body and governance key")]
    InvalidSignature,
    #[error("Signature bytes do not decode to a valid Schnorr signature")]
    BadSignatureEncoding,
    #[error("Failed to serialise directive body: {0}")]
    Serialise(String),
    #[error("Signing failure: {0:?}")]
    Sign(SchnorrSignatureError),
}
