//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, str::FromStr};

use anyhow::anyhow;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use multiaddr::Multiaddr;
use ootle_byte_type::ToByteType;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::hex::Hex};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

/// Parsed information from a peer seed string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedPeer {
    public_key: Option<RistrettoPublicKeyBytes>,
    address: Multiaddr,
}

impl SeedPeer {
    pub fn public_key(&self) -> Option<&RistrettoPublicKeyBytes> {
        self.public_key.as_ref()
    }

    pub fn address(&self) -> &Multiaddr {
        &self.address
    }

    pub fn into_address(self) -> Multiaddr {
        self.address
    }

    pub fn to_peer_id(&self) -> Option<PeerId> {
        let pk = self.public_key.as_ref()?;
        let pk = identity::PublicKey::from(
            // invariant: we only construct SeedPeer with valid public keys
            identity::sr25519::PublicKey::try_from_bytes(pk.as_bytes()).expect("invariant: valid public key"),
        );
        Some(pk.to_peer_id())
    }
}

impl FromStr for SeedPeer {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.starts_with("/") {
            // This is a Multiaddr, so we assume the public key is not included
            let address: Multiaddr = s.parse().map_err(|err| anyhow!("Invalid address {err}"))?;
            if address.is_empty() {
                return Err(anyhow!("Invalid address: empty multiaddr"));
            }
            return Ok(SeedPeer {
                public_key: None,
                address,
            });
        }

        let (pk, address) = s.split_once("::").ok_or_else(|| anyhow!("Invalid seed peer format"))?;
        // Ensure that the public key is valid
        let public_key = RistrettoPublicKey::from_hex(pk).map_err(|err| anyhow!("Invalid public key {err}"))?;
        let address: Multiaddr = address.parse().map_err(|err| anyhow!("Invalid address {err}"))?;
        if address.is_empty() {
            return Err(anyhow!("Invalid address: empty multiaddr"));
        }
        Ok(SeedPeer {
            public_key: Some(public_key.to_byte_type()),
            address,
        })
    }
}

impl Display for SeedPeer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.public_key {
            Some(pk) => write!(f, "{}::{}", pk, self.address),
            None => write!(f, "{}", self.address),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_with_public_key() {
        let s = " 0000000000000000000000000000000000000000000000000000000000000000::/ip4/127.0.0.1/tcp/8080";

        let seed_peer = SeedPeer::from_str(s).expect("Failed to parse seed peer");
        assert_eq!(
            seed_peer.public_key().unwrap().to_string(),
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(seed_peer.address().to_string(), "/ip4/127.0.0.1/tcp/8080");

        assert!(seed_peer.to_peer_id().is_some());
    }

    #[test]
    fn it_fails_to_parse_with_invalid_public_key() {
        let s = "deadbeaf00000000000deadbeaf0000000000000000000000000000000000000::/ip4/127.0.0.1/tcp/8080";

        let err = SeedPeer::from_str(s).unwrap_err();
        assert!(err.to_string().contains("Invalid public key"));
    }

    #[test]
    fn it_parses_without_public_key() {
        let s = "/ip4/127.0.0.1/tcp/8080";

        let seed_peer = SeedPeer::from_str(s).expect("Failed to parse seed peer");
        assert!(seed_peer.public_key().is_none());
        assert_eq!(seed_peer.address().to_string(), s);
    }

    #[test]
    fn it_parses_ipv6() {
        let s = "0000000000000000000000000000000000000000000000000000000000000000::/ip6/::1/tcp/8080";

        let seed_peer = SeedPeer::from_str(s).expect("Failed to parse seed peer");
        assert_eq!(
            seed_peer.public_key().unwrap().to_string(),
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(seed_peer.address().to_string(), "/ip6/::1/tcp/8080");
    }

    #[test]
    fn display_round_trips_with_and_without_public_key() {
        let with_pk = "0000000000000000000000000000000000000000000000000000000000000000::/ip4/127.0.0.1/tcp/8080";
        let parsed = SeedPeer::from_str(with_pk).unwrap();
        assert_eq!(parsed.to_string(), with_pk);
        assert_eq!(SeedPeer::from_str(&parsed.to_string()).unwrap(), parsed);

        let without_pk = "/ip4/127.0.0.1/tcp/8080";
        let parsed_no_pk = SeedPeer::from_str(without_pk).unwrap();
        assert_eq!(parsed_no_pk.to_string(), without_pk);
        assert_eq!(SeedPeer::from_str(&parsed_no_pk.to_string()).unwrap(), parsed_no_pk);
    }

    #[test]
    fn it_rejects_empty_address_after_separator() {
        let s = "0000000000000000000000000000000000000000000000000000000000000000::";
        let err = SeedPeer::from_str(s).unwrap_err();
        assert!(err.to_string().contains("Invalid address"));
    }
}
