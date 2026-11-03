//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{fmt, marker::PhantomData};

use crate::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes, Sr25519SignatureBytes};

pub trait SignatureDomain {
    fn domain() -> &'static [u8];
}

/// A signature domain that is the empty byte string. Verifying under it fails the transaction.
///
/// A [`Signature`] carries no domain on the wire, so this type serves for passing a signature to a
/// template that verifies it under a domain of its own. To verify, define a domain with the
/// `custom_signature_domain!` macro.
///
/// # Example
/// ```rust,ignore
/// custom_signature_domain!(MyAppDomain, b"MyAppSignatureDomain");
/// ```
pub struct NoSignatureDomain;

/// What a template signature is bound to besides its domain and the network.
///
/// The engine resolves the scope from the call that verifies, so a template can bind a signature only
/// to itself or its own component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SignatureScope {
    /// Any template verifying under the same domain accepts the signature, for attestations that more
    /// than one template consumes.
    #[default]
    #[n(0)]
    Shared,
    /// Only the template that verifies accepts the signature.
    #[n(1)]
    CurrentTemplate,
    /// Only the component that verifies accepts the signature. Verifying outside a component method fails the
    /// transaction.
    #[n(2)]
    CurrentComponent,
}

impl SignatureDomain for NoSignatureDomain {
    fn domain() -> &'static [u8] {
        b""
    }
}

#[macro_export]
macro_rules! custom_signature_domain {
    ($name:ident, $domain:expr) => {
        pub struct $name;

        impl $crate::crypto::SignatureDomain for $name {
            fn domain() -> &'static [u8] {
                $domain
            }
        }
    };
}

/// A signature with an associated signature domain.
///
/// The engine verifies it over a message it derives from the network, the scope, the domain and the
/// template's message (`tari_ootle_common_types::template_signature_message`), so a signer computes that
/// message first. An sr25519 signer must also accept the template signing context; one fixed to
/// `substrate` cannot produce a template signature.
///
/// # Example
/// ```rust,ignore
/// // Define a custom signature domain
/// custom_signature_domain!(MyAppDomain, b"MyAppSignatureDomain");
///
/// // Create a signature type with the custom domain
/// let paylaod = SignaturePayload::RistrettoSchnorrBlake2b(schnorr_signature_bytes);
/// let signature = Signature::<MyAppDomain>::new(paylaod);
/// // Verify the signature
/// let is_valid = signature.verify(&public_key, &message);
/// ```
#[derive(Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Signature<D> {
    #[n(0)]
    payload: SignaturePayload,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cbor(skip)]
    _domain: PhantomData<D>,
}

impl<D: SignatureDomain, T: Into<SignaturePayload>> From<T> for Signature<D> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SignaturePayload {
    /// A Ristretto Schnorr signature whose challenge is
    /// `tari_ootle_common_types::RistrettoSchnorrBlake2bVerifier::compute_challenge`: a hash under the
    /// template signature domain over the public nonce, the public key and the template signature message.
    #[n(0)]
    RistrettoSchnorrBlake2b(#[n(0)] SchnorrSignatureBytes),
    /// A standard sr25519 (schnorrkel) signature under the engine's template signing context, over a
    /// message the engine derives from the network, the scope, the signature domain and the template's
    /// message (`tari_ootle_common_types::template_signature_message`). Transaction, node identity and
    /// other protocol signatures sign under contexts of their own, so none of them verifies as one. A
    /// signer fixed to the `substrate` context cannot produce one.
    #[n(1)]
    Sr25519(#[n(0)] Sr25519SignatureBytes),
}

impl SignaturePayload {
    pub fn ristretto_schnorr_blake2b(&self) -> Option<&SchnorrSignatureBytes> {
        match self {
            SignaturePayload::RistrettoSchnorrBlake2b(sig) => Some(sig),
            SignaturePayload::Sr25519(_) => None,
        }
    }

    pub fn sr25519(&self) -> Option<&Sr25519SignatureBytes> {
        match self {
            SignaturePayload::Sr25519(sig) => Some(sig),
            SignaturePayload::RistrettoSchnorrBlake2b(_) => None,
        }
    }
}

impl From<SchnorrSignatureBytes> for SignaturePayload {
    fn from(value: SchnorrSignatureBytes) -> Self {
        SignaturePayload::RistrettoSchnorrBlake2b(value)
    }
}

impl From<Sr25519SignatureBytes> for SignaturePayload {
    fn from(value: Sr25519SignatureBytes) -> Self {
        SignaturePayload::Sr25519(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PublicKey {
    #[default]
    #[n(0)]
    Zero,
    #[n(1)]
    Ristretto25519(#[n(0)] RistrettoPublicKeyBytes),
}

impl PublicKey {
    pub fn ristretto25519(&self) -> Option<&RistrettoPublicKeyBytes> {
        match self {
            Self::Zero => None,
            Self::Ristretto25519(pk) => Some(pk),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Zero => &[],
            Self::Ristretto25519(pk) => pk.as_bytes(),
        }
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => write!(f, "Zero"),
            Self::Ristretto25519(pk) => write!(f, "Ristretto25519({})", pk),
        }
    }
}

impl From<RistrettoPublicKeyBytes> for PublicKey {
    fn from(value: RistrettoPublicKeyBytes) -> Self {
        Self::Ristretto25519(value)
    }
}

impl<D: SignatureDomain> Signature<D> {
    pub fn new<T: Into<SignaturePayload>>(payload: T) -> Self {
        Self {
            payload: payload.into(),
            _domain: PhantomData,
        }
    }
}

impl<D> Signature<D> {
    pub fn payload(&self) -> &SignaturePayload {
        &self.payload
    }
}
