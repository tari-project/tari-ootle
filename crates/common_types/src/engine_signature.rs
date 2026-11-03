//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Signatures that templates verify.
//!
//! A template names a signature domain, a scope and a message, and the engine verifies over a message it
//! derives itself: [`template_signature_message`], a hash of the [`TemplateSignatureBinding`] (the
//! network and the template or component the scope resolves to), the template's domain and the template's
//! message under the [`TemplateSignatureHashDomain`]. sr25519 signatures sign it under
//! [`TEMPLATE_SIGNING_CONTEXT`] and Schnorr signatures commit to it in their challenge, so a template
//! signature verifies only for the binding, domain and message it was made for, and transaction, node
//! identity and other protocol signatures, which sign under contexts of their own, verify as none.

use blake2::Blake2b;
use digest::{FixedOutput, consts::U64};
use ootle_byte_type::{ConvertFromByteType, FromByteType, ToByteType};
use ootle_network::Network;
use tari_crypto::{
    hash_domain,
    hashing::DomainSeparatedHasher,
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_sr25519::{SigningContext, Sr25519SignatureBytes};
use tari_template_lib_types::{
    ComponentAddress,
    TemplateAddress,
    crypto::{PublicKey, RistrettoPublicKeyBytes, SchnorrSignatureBytes, SignaturePayload},
};

hash_domain!(TemplateSignatureHashDomain, "com.tari.ootle.template_signature", 1);

/// The sr25519 signing context of every signature a template verifies.
pub const TEMPLATE_SIGNING_CONTEXT: &[u8] = b"com.tari.ootle.template_signature.v1";

type TemplateSignatureHasher = DomainSeparatedHasher<Blake2b<U64>, TemplateSignatureHashDomain>;

/// The template or component a template signature is bound to: a
/// [`SignatureScope`](tari_template_lib_types::crypto::SignatureScope) resolved against the call that verifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSignatureScope {
    Shared,
    Template(TemplateAddress),
    Component(ComponentAddress),
}

/// Everything a template signature is bound to besides its domain and message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateSignatureBinding {
    pub network: Network,
    pub scope: TemplateSignatureScope,
}

impl TemplateSignatureBinding {
    pub const fn shared(network: Network) -> Self {
        Self {
            network,
            scope: TemplateSignatureScope::Shared,
        }
    }

    pub const fn template(network: Network, template: TemplateAddress) -> Self {
        Self {
            network,
            scope: TemplateSignatureScope::Template(template),
        }
    }

    pub const fn component(network: Network, component: ComponentAddress) -> Self {
        Self {
            network,
            scope: TemplateSignatureScope::Component(component),
        }
    }
}

/// The message a template signature signs for `message` under the template's signature `domain`, bound
/// to `binding`. Every input is length-prefixed, so no other binding, domain and message produce it.
pub fn template_signature_message(binding: &TemplateSignatureBinding, domain: &[u8], message: &[u8]) -> [u8; 64] {
    let (scope_tag, scope_address): (u8, &[u8]) = match &binding.scope {
        TemplateSignatureScope::Shared => (0, &[]),
        TemplateSignatureScope::Template(template) => (1, template.as_slice()),
        TemplateSignatureScope::Component(component) => (2, component.as_bytes()),
    };
    let mut out = [0u8; 64];
    TemplateSignatureHasher::new_with_label("message")
        .chain([binding.network.as_byte()])
        .chain([scope_tag])
        .chain(scope_address)
        .chain(domain)
        .chain(message)
        .finalize_into((&mut out).into());
    out
}

/// Signs `message` with sr25519 under the template signature `domain`, bound to `binding`, as a template
/// verifies it.
pub fn sign_template_message_sr25519(
    secret_key: &RistrettoSecretKey,
    binding: &TemplateSignatureBinding,
    domain: &[u8],
    message: &[u8],
) -> Sr25519SignatureBytes {
    SigningContext::new(TEMPLATE_SIGNING_CONTEXT)
        .sign(secret_key, &template_signature_message(binding, domain, message))
}

/// Signs `message` with a Ristretto Schnorr signature under the template signature `domain`, bound to
/// `binding`, as a template verifies it.
pub fn sign_template_message_schnorr(
    secret_key: &RistrettoSecretKey,
    binding: &TemplateSignatureBinding,
    domain: &[u8],
    message: &[u8],
) -> SchnorrSignatureBytes {
    let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let public_key = RistrettoPublicKey::from_secret_key(secret_key);
    let challenge = RistrettoSchnorrBlake2bVerifier::compute_challenge(
        binding,
        domain,
        message,
        &public_key.to_byte_type(),
        &public_nonce.to_byte_type(),
    );
    RistrettoSchnorr::sign_raw_uniform(secret_key, nonce, &challenge)
        .expect("a 64-byte challenge is a valid uniform scalar input")
        .to_byte_type()
}

pub trait GetVerifier {
    fn get_verifier(&self) -> &dyn Verifier;
}

impl GetVerifier for SignaturePayload {
    fn get_verifier(&self) -> &dyn Verifier {
        match self {
            SignaturePayload::RistrettoSchnorrBlake2b(_) => &RistrettoSchnorrBlake2bVerifier,
            SignaturePayload::Sr25519(_) => &Sr25519Verifier,
        }
    }
}

/// Verifies a template signature over `message` under the template's signature `domain`, bound to `binding`.
///
/// The empty domain verifies nothing: a domain names what a signature is for, and the empty one names
/// nothing a template could tell apart from another's.
pub trait Verifier {
    #[must_use]
    fn verify(
        &self,
        binding: &TemplateSignatureBinding,
        domain: &[u8],
        message: &[u8],
        public_key: &PublicKey,
        signature: &SignaturePayload,
    ) -> bool;
}

/// A verifier for Ristretto Schnorr signatures with a Blake2b challenge over the template signature message.
pub struct RistrettoSchnorrBlake2bVerifier;

impl RistrettoSchnorrBlake2bVerifier {
    /// The challenge of a Schnorr template signature with public nonce `nonce` by `public_key`, over
    /// [`template_signature_message`]`(binding, domain, message)`.
    pub fn compute_challenge(
        binding: &TemplateSignatureBinding,
        domain: &[u8],
        message: &[u8],
        public_key: &RistrettoPublicKeyBytes,
        nonce: &RistrettoPublicKeyBytes,
    ) -> [u8; 64] {
        let mut out = [0u8; 64];
        TemplateSignatureHasher::new_with_label("challenge")
            .chain(nonce.as_bytes())
            .chain(public_key.as_bytes())
            .chain(template_signature_message(binding, domain, message))
            .finalize_into((&mut out).into());
        out
    }
}

impl Verifier for RistrettoSchnorrBlake2bVerifier {
    fn verify(
        &self,
        binding: &TemplateSignatureBinding,
        domain: &[u8],
        message: &[u8],
        public_key: &PublicKey,
        signature: &SignaturePayload,
    ) -> bool {
        if domain.is_empty() {
            return false;
        }
        // A caller chooses both the key and the payload, and `PublicKey`/`SignaturePayload` are open enums whose
        // other variants are valid encodings, so a variant this verifier cannot use is a failed verification.
        let Some(sig) = signature.ristretto_schnorr_blake2b() else {
            return false;
        };
        let Some(ristretto_public_key) = public_key.ristretto25519() else {
            return false;
        };

        let Ok(pk) = ristretto_public_key.try_from_byte_type() else {
            return false;
        };

        if sig.public_nonce().is_zero() {
            return false;
        }

        let Ok(sig) = RistrettoSchnorr::convert_from_byte_type(sig) else {
            return false;
        };

        let challenge = Self::compute_challenge(
            binding,
            domain,
            message,
            ristretto_public_key,
            &sig.get_public_nonce().to_byte_type(),
        );
        sig.verify_raw_uniform(&pk, &challenge)
    }
}

/// A verifier for standard sr25519 signatures under [`TEMPLATE_SIGNING_CONTEXT`] over the template
/// signature message.
pub struct Sr25519Verifier;

impl Verifier for Sr25519Verifier {
    fn verify(
        &self,
        binding: &TemplateSignatureBinding,
        domain: &[u8],
        message: &[u8],
        public_key: &PublicKey,
        signature: &SignaturePayload,
    ) -> bool {
        if domain.is_empty() {
            return false;
        }
        let Some(sig) = signature.sr25519() else {
            return false;
        };
        let Some(public_key) = public_key.ristretto25519() else {
            return false;
        };
        SigningContext::new(TEMPLATE_SIGNING_CONTEXT).verify(
            sig,
            public_key,
            &template_signature_message(binding, domain, message),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINDING: TemplateSignatureBinding = TemplateSignatureBinding::shared(Network::LocalNet);
    const DOMAIN: &[u8] = b"Test domain";
    const MESSAGE: &[u8] = b"Hello, world!";

    fn keypair() -> (RistrettoSecretKey, PublicKey) {
        let (secret_key, public_key) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        (secret_key, PublicKey::Ristretto25519(public_key.to_byte_type()))
    }

    fn other_bindings() -> [TemplateSignatureBinding; 4] {
        [
            TemplateSignatureBinding::shared(Network::Esmeralda),
            TemplateSignatureBinding::template(Network::LocalNet, TemplateAddress::from_array([1; 32])),
            TemplateSignatureBinding::component(Network::LocalNet, ComponentAddress::from_array([1; 32])),
            TemplateSignatureBinding::component(Network::LocalNet, ComponentAddress::from_array([2; 32])),
        ]
    }

    /// Each payload verifies for exactly the binding, domain and message it signed.
    fn assert_bound(payload: &SignaturePayload, public_key: &PublicKey) {
        let verifier = payload.get_verifier();
        assert!(verifier.verify(&BINDING, DOMAIN, MESSAGE, public_key, payload));
        for binding in other_bindings() {
            assert!(!verifier.verify(&binding, DOMAIN, MESSAGE, public_key, payload));
        }
        assert!(!verifier.verify(&BINDING, b"Another domain", MESSAGE, public_key, payload));
        assert!(!verifier.verify(&BINDING, DOMAIN, b"Another message", public_key, payload));
        let (_, other_key) = keypair();
        assert!(!verifier.verify(&BINDING, DOMAIN, MESSAGE, &other_key, payload));
    }

    #[test]
    fn a_schnorr_signature_verifies_only_for_what_it_signed() {
        let (secret_key, public_key) = keypair();
        let payload = sign_template_message_schnorr(&secret_key, &BINDING, DOMAIN, MESSAGE).into();
        assert_bound(&payload, &public_key);
    }

    #[test]
    fn an_sr25519_signature_verifies_only_for_what_it_signed() {
        let (secret_key, public_key) = keypair();
        let payload = sign_template_message_sr25519(&secret_key, &BINDING, DOMAIN, MESSAGE).into();
        assert_bound(&payload, &public_key);
    }

    #[test]
    fn a_scoped_signature_verifies_only_in_its_scope() {
        let (secret_key, public_key) = keypair();
        let [_, template, component, other_component] = other_bindings();
        for binding in [template, component] {
            let payload = SignaturePayload::from(sign_template_message_sr25519(&secret_key, &binding, DOMAIN, MESSAGE));
            assert!(Sr25519Verifier.verify(&binding, DOMAIN, MESSAGE, &public_key, &payload));
            for other in [BINDING, template, component, other_component] {
                if other != binding {
                    assert!(!Sr25519Verifier.verify(&other, DOMAIN, MESSAGE, &public_key, &payload));
                }
            }
        }
    }

    #[test]
    fn a_payload_verifies_only_with_its_own_scheme() {
        let (secret_key, public_key) = keypair();
        let sr25519 = SignaturePayload::from(sign_template_message_sr25519(&secret_key, &BINDING, DOMAIN, MESSAGE));
        let schnorr = SignaturePayload::from(sign_template_message_schnorr(&secret_key, &BINDING, DOMAIN, MESSAGE));
        assert!(!RistrettoSchnorrBlake2bVerifier.verify(&BINDING, DOMAIN, MESSAGE, &public_key, &sr25519));
        assert!(!Sr25519Verifier.verify(&BINDING, DOMAIN, MESSAGE, &public_key, &schnorr));
    }

    #[test]
    fn the_empty_domain_verifies_nothing() {
        let (secret_key, public_key) = keypair();
        for payload in [
            SignaturePayload::from(sign_template_message_sr25519(&secret_key, &BINDING, b"", MESSAGE)),
            SignaturePayload::from(sign_template_message_schnorr(&secret_key, &BINDING, b"", MESSAGE)),
        ] {
            assert!(
                !payload
                    .get_verifier()
                    .verify(&BINDING, b"", MESSAGE, &public_key, &payload)
            );
        }
    }

    /// A signature some other protocol makes with the same key, under its own context over the template's
    /// message bytes, verifies as no template signature.
    #[test]
    fn a_signature_under_another_context_verifies_nothing() {
        let (secret_key, public_key) = keypair();
        for context in [&b"substrate"[..], b"libp2p", DOMAIN] {
            let raw = SignaturePayload::Sr25519(SigningContext::new(context).sign(&secret_key, MESSAGE));
            let derived = SignaturePayload::Sr25519(
                SigningContext::new(context).sign(&secret_key, &template_signature_message(&BINDING, DOMAIN, MESSAGE)),
            );
            for payload in [raw, derived] {
                assert!(
                    !payload
                        .get_verifier()
                        .verify(&BINDING, DOMAIN, MESSAGE, &public_key, &payload)
                );
            }
        }
    }

    #[test]
    fn domain_and_message_boundaries_are_unambiguous() {
        assert_ne!(
            template_signature_message(&BINDING, b"ab", b"c"),
            template_signature_message(&BINDING, b"a", b"bc")
        );
    }
}
