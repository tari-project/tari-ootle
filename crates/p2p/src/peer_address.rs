//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, hash::Hash};

use libp2p_identity as identity;
use libp2p_identity::PeerId;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_ootle_common_types::{DerivableFromPublicKey, NodeAddressable};
#[cfg(feature = "ts")]
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct PeerAddress(#[cfg_attr(feature = "ts", ts(type = "string"))] PeerId);

impl PeerAddress {
    pub fn as_peer_id(&self) -> PeerId {
        self.0
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes()
    }
}

impl ToPeerId for PeerAddress {
    fn to_peer_id(&self) -> PeerId {
        self.0
    }
}

impl NodeAddressable for PeerAddress {
    fn zero() -> Self {
        // type, len, data
        Self(PeerId::from_bytes(&[0u8, 1, 0]).unwrap())
    }

    fn try_from_public_key(public_key: &RistrettoPublicKey) -> Option<Self> {
        Some(public_key.clone().into())
    }
}

impl From<identity::PublicKey> for PeerAddress {
    fn from(peer_pk: identity::PublicKey) -> Self {
        peer_pk.to_peer_id().into()
    }
}

impl From<&identity::PublicKey> for PeerAddress {
    fn from(peer_pk: &identity::PublicKey) -> Self {
        peer_pk.to_peer_id().into()
    }
}

impl Display for PeerAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_peer_id())
    }
}

impl From<RistrettoPublicKey> for PeerAddress {
    fn from(pk: RistrettoPublicKey) -> Self {
        Self(public_key_to_peer_id(pk))
    }
}

impl From<PeerId> for PeerAddress {
    fn from(peer_id: PeerId) -> Self {
        Self(peer_id)
    }
}

impl From<&PeerId> for PeerAddress {
    fn from(peer_id: &PeerId) -> Self {
        Self(*peer_id)
    }
}

impl PartialEq<PeerId> for PeerAddress {
    fn eq(&self, other: &PeerId) -> bool {
        self.as_peer_id() == *other
    }
}

impl DerivableFromPublicKey for PeerAddress {}

pub fn public_key_to_peer_id(public_key: RistrettoPublicKey) -> PeerId {
    let public_key = identity::sr25519::PublicKey::try_from_bytes(public_key.as_bytes())
        .expect("a RistrettoPublicKey is a canonically encoded point");
    identity::PublicKey::from(public_key).to_peer_id()
}

pub trait ToPeerId {
    fn to_peer_id(&self) -> PeerId;
}

#[cfg(test)]
mod tests {
    use tari_crypto::{keys::PublicKey, ristretto::RistrettoSecretKey, tari_utilities::hex::Hex};

    use super::*;

    #[test]
    fn zero() {
        let _ = PeerAddress::zero();
    }

    #[test]
    fn check_conversions() {
        let (_, pk) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let peer_address = PeerAddress::try_from_public_key(&pk).unwrap();
        let peer_id = peer_address.as_peer_id();
        let peer_address2 = PeerAddress::from(peer_id);
        assert_eq!(peer_address, peer_address2);
        let peer_id2 = peer_address.as_peer_id();
        assert_eq!(peer_id2, peer_id);
    }

    /// A node's own PeerId comes from its secret key (`Keypair::sr25519_from_bytes` in the VN and indexer
    /// bootstrap); everyone else derives it from the registered public key. Peers are only reachable if both
    /// agree.
    #[test]
    fn peer_id_from_public_key_matches_the_keypair() {
        let (secret_key, public_key) = RistrettoPublicKey::random_keypair(&mut rand::rng());
        let keypair = identity::Keypair::sr25519_from_bytes(secret_key.as_bytes().to_vec()).unwrap();

        assert_eq!(public_key_to_peer_id(public_key), keypair.public().to_peer_id());
    }

    /// The PeerId of secret key `[7; 32]`: the identity multihash of the protobuf key `08 05 12 20 ‖ public key`,
    /// with sr25519 as libp2p key type 5. PeerIds are what nodes dial, so a fork bump must not change them.
    #[test]
    fn peer_id_is_stable_across_fork_bumps() {
        const PEER_ID: &str = "12D3LCx9PHvYgUhBcUcYdW4VLTyccHZ1rSvxPNnzoh7W9tEkx663";
        let secret_key = RistrettoSecretKey::from_canonical_bytes(&[7u8; 32]).unwrap();
        let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
        assert_eq!(
            public_key.to_hex(),
            "aaf82404e5f7bfa7352ce093e4aabe82435385d64aa870090a56d1aa36169800"
        );

        assert_eq!(public_key_to_peer_id(public_key).to_string(), PEER_ID);
        let keypair = identity::Keypair::sr25519_from_bytes([7u8; 32].to_vec()).unwrap();
        assert_eq!(keypair.public().to_peer_id().to_string(), PEER_ID);
    }
}
