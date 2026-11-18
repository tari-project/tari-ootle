//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The authorizer set every [`SchemaVersion::V2`](crate::SchemaVersion::V2) authorization signature
//! commits to.
//!
//! A V2 authorization signs over the public keys of the transaction's whole authorization list, its
//! own key included. The sealer assembles that list, so binding it into each authorization is what
//! stops the sealer adding, dropping, repeating or reordering authorizations: any change to the key
//! list changes the message, and every authorization over the original list stops verifying.
//!
//! A valid V2 list is strictly ascending by public key, which fixes one encoding per key set.

use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::TransactionSignature;

/// The public keys of a transaction's authorizations, ascending and without repeats.
///
/// Built on the signing side, before any authorization exists: every authorizer must know the
/// whole set before it signs. It borsh-encodes as a `Vec` of keys, identically to the key list
/// a verifier reads off the signatures (see [`SignatureKeys`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, borsh::BorshSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "Vec<RistrettoPublicKeyBytes>"))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AuthorizerSet(Vec<RistrettoPublicKeyBytes>);

impl AuthorizerSet {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn keys(&self) -> &[RistrettoPublicKeyBytes] {
        &self.0
    }

    pub fn contains(&self, key: &RistrettoPublicKeyBytes) -> bool {
        self.0.binary_search(key).is_ok()
    }

    /// Adds `key`, keeping the set ascending. Returns false when the key is already present.
    pub fn insert(&mut self, key: RistrettoPublicKeyBytes) -> bool {
        match self.0.binary_search(&key) {
            Ok(_) => false,
            Err(pos) => {
                self.0.insert(pos, key);
                true
            },
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &RistrettoPublicKeyBytes> + '_ {
        self.0.iter()
    }

    /// The key set of an authorization list, which matches the list exactly when the list is
    /// canonical (see [`is_canonical_authorization_list`]).
    pub fn from_signatures(signatures: &[TransactionSignature]) -> Self {
        signatures.iter().map(|sig| *sig.public_key()).collect()
    }
}

impl From<Vec<RistrettoPublicKeyBytes>> for AuthorizerSet {
    fn from(mut keys: Vec<RistrettoPublicKeyBytes>) -> Self {
        keys.sort_unstable();
        keys.dedup();
        Self(keys)
    }
}

impl FromIterator<RistrettoPublicKeyBytes> for AuthorizerSet {
    fn from_iter<I: IntoIterator<Item = RistrettoPublicKeyBytes>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<'a> FromIterator<&'a RistrettoPublicKeyBytes> for AuthorizerSet {
    fn from_iter<I: IntoIterator<Item = &'a RistrettoPublicKeyBytes>>(iter: I) -> Self {
        iter.into_iter().copied().collect()
    }
}

impl IntoIterator for AuthorizerSet {
    type IntoIter = std::vec::IntoIter<RistrettoPublicKeyBytes>;
    type Item = RistrettoPublicKeyBytes;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a AuthorizerSet {
    type IntoIter = std::slice::Iter<'a, RistrettoPublicKeyBytes>;
    type Item = &'a RistrettoPublicKeyBytes;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// What an authorization signature is made under: the seal signer and the full authorizer set.
#[derive(Debug, Clone, Copy)]
pub struct AuthorizationContext<'a> {
    seal_signer: &'a RistrettoPublicKeyBytes,
    authorizers: &'a AuthorizerSet,
}

impl<'a> AuthorizationContext<'a> {
    pub const fn new(seal_signer: &'a RistrettoPublicKeyBytes, authorizers: &'a AuthorizerSet) -> Self {
        Self {
            seal_signer,
            authorizers,
        }
    }

    pub const fn seal_signer(&self) -> &'a RistrettoPublicKeyBytes {
        self.seal_signer
    }

    pub const fn authorizers(&self) -> &'a AuthorizerSet {
        self.authorizers
    }
}

/// True when `signatures` is strictly ascending by public key: no key repeats and the order is the
/// one canonical order.
pub fn is_canonical_authorization_list(signatures: &[TransactionSignature]) -> bool {
    signatures
        .windows(2)
        .all(|pair| matches!(pair, [a, b] if a.public_key() < b.public_key()))
}

/// Orders `signatures` canonically, keeping the first signature of a repeated key.
pub(crate) fn canonicalize_authorization_list(signatures: &mut Vec<TransactionSignature>) {
    signatures.sort_by_key(|sig| *sig.public_key());
    signatures.dedup_by_key(|sig| *sig.public_key());
}

/// The public keys of an authorization list, borsh-encoded as an [`AuthorizerSet`] holding the
/// same keys in the same order encodes. A verifier chains this into the authorization message and
/// the transaction id, so it reads the keys off the list without collecting them.
pub(crate) struct SignatureKeys<'a>(pub &'a [TransactionSignature]);

impl borsh::BorshSerialize for SignatureKeys<'_> {
    fn serialize<W: borsh::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        let len = u32::try_from(self.0.len()).map_err(|_| borsh::io::ErrorKind::InvalidData)?;
        borsh::BorshSerialize::serialize(&len, writer)?;
        for signature in self.0 {
            borsh::BorshSerialize::serialize(signature.public_key(), writer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::signature::Sr25519SignatureBytes;

    use super::*;

    fn key(byte: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[byte; 32]).unwrap()
    }

    fn sig(byte: u8) -> TransactionSignature {
        TransactionSignature::new(key(byte), Sr25519SignatureBytes::from_array([byte; 64]))
    }

    #[test]
    fn set_is_ascending_and_deduplicated() {
        let set: AuthorizerSet = [key(3), key(1), key(3), key(2)].into_iter().collect();
        assert_eq!(set.keys(), &[key(1), key(2), key(3)]);
        assert!(set.contains(&key(2)));
        assert!(!set.contains(&key(4)));

        let mut set = set;
        assert!(set.insert(key(0)));
        assert!(!set.insert(key(2)));
        assert_eq!(set.keys(), &[key(0), key(1), key(2), key(3)]);
    }

    /// Signers hash an `AuthorizerSet` and verifiers hash `SignatureKeys` over the list they
    /// received: the two must encode byte for byte alike, or no authorization would verify.
    #[test]
    fn signature_keys_encode_as_the_authorizer_set() {
        for list in [vec![], vec![sig(1)], vec![sig(1), sig(2), sig(9)]] {
            let set = AuthorizerSet::from_signatures(&list);
            assert_eq!(
                borsh::to_vec(&SignatureKeys(&list)).unwrap(),
                borsh::to_vec(&set).unwrap(),
                "{} keys",
                list.len()
            );
        }
    }

    /// An empty key list encodes as an empty signature list does.
    #[test]
    fn empty_signature_keys_encode_as_an_empty_signature_list() {
        assert_eq!(
            borsh::to_vec(&SignatureKeys(&[])).unwrap(),
            borsh::to_vec(&Vec::<TransactionSignature>::new()).unwrap(),
        );
    }

    #[test]
    fn canonical_list_is_strictly_ascending() {
        assert!(is_canonical_authorization_list(&[]));
        assert!(is_canonical_authorization_list(&[sig(1)]));
        assert!(is_canonical_authorization_list(&[sig(1), sig(2)]));
        assert!(!is_canonical_authorization_list(&[sig(2), sig(1)]), "descending");
        assert!(!is_canonical_authorization_list(&[sig(1), sig(1)]), "repeated key");
    }

    #[test]
    fn canonicalize_sorts_and_keeps_the_first_of_a_repeated_key() {
        let first = sig(1);
        let repeat = TransactionSignature::new(key(1), Sr25519SignatureBytes::from_array([0xaa; 64]));
        let mut list = vec![sig(3), first.clone(), repeat, sig(2)];
        canonicalize_authorization_list(&mut list);
        assert_eq!(list, vec![first, sig(2), sig(3)]);
    }
}
