//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_address::{OotleAddress, PayRef};
use tari_ootle_common_types::Network;

use crate::error::OotleWasmError;

/// The two secret keys (owner and view) that form an Ootle wallet identity.
#[derive(Debug, Clone)]
pub struct OotleSecretKeyResult {
    /// The owner (spending) secret key bytes.
    pub owner_key: Vec<u8>,
    /// The view-only secret key bytes.
    pub view_key: Vec<u8>,
}

/// The two public keys derived from the secret keys.
#[derive(Debug, Clone)]
pub struct OotlePublicKeyResult {
    /// The owner (spending) public key bytes.
    pub owner_key: Vec<u8>,
    /// The view-only public key bytes.
    pub view_key: Vec<u8>,
}

/// Generate a new random pair of Ootle secret keys (owner + view).
pub fn generate_ootle_secret_key() -> OotleSecretKeyResult {
    let owner_key = RistrettoSecretKey::random(&mut OsRng);
    let view_key = RistrettoSecretKey::random(&mut OsRng);
    OotleSecretKeyResult {
        owner_key: owner_key.as_bytes().to_vec(),
        view_key: view_key.as_bytes().to_vec(),
    }
}

/// Derive the public keys from a pair of Ootle secret keys.
pub fn ootle_public_key_from_secret_key(secret_key: &OotleSecretKeyResult) -> Result<OotlePublicKeyResult, OotleWasmError> {
    let owner_secret = RistrettoSecretKey::from_canonical_bytes(&secret_key.owner_key)
        .map_err(|e| OotleWasmError::InvalidSecretKey(e.to_string()))?;
    let view_secret = RistrettoSecretKey::from_canonical_bytes(&secret_key.view_key)
        .map_err(|e| OotleWasmError::InvalidSecretKey(e.to_string()))?;

    let owner_public = RistrettoPublicKey::from_secret_key(&owner_secret);
    let view_public = RistrettoPublicKey::from_secret_key(&view_secret);

    Ok(OotlePublicKeyResult {
        owner_key: owner_public.as_bytes().to_vec(),
        view_key: view_public.as_bytes().to_vec(),
    })
}

/// Parsed components of an Ootle address.
#[derive(Debug, Clone)]
pub struct ParsedOotleAddress {
    /// The owner (spending) public key bytes.
    pub owner_key: Vec<u8>,
    /// The view-only public key bytes.
    pub view_key: Vec<u8>,
    /// The network byte.
    pub network: u8,
    /// Optional pay reference / memo bytes.
    pub memo: Option<Vec<u8>>,
}

/// Parse a bech32m Ootle address string into its components.
pub fn parse_ootle_address(address: &str) -> Result<ParsedOotleAddress, OotleWasmError> {
    let parsed = OotleAddress::decode_bech32(address).map_err(|e| OotleWasmError::InvalidAddress(e.to_string()))?;

    Ok(ParsedOotleAddress {
        owner_key: parsed.account_public_key().as_bytes().to_vec(),
        view_key: parsed.view_only_key().as_bytes().to_vec(),
        network: parsed.network() as u8,
        memo: parsed.pay_ref().map(|pr| pr.as_bytes().to_vec()),
    })
}

/// Generate an Ootle address (bech32m string) from public keys.
///
/// `network` is the network byte (see `Network` enum).
/// `memo` is an optional pay reference (max 64 bytes).
pub fn generate_ootle_address(
    owner_public_key: &[u8],
    view_public_key: &[u8],
    network: u8,
    memo: Option<&[u8]>,
) -> Result<String, OotleWasmError> {
    let network = Network::try_from(network).map_err(|e| OotleWasmError::InvalidNetwork(e.to_string()))?;

    let owner_pk = RistrettoPublicKey::from_canonical_bytes(owner_public_key)
        .map_err(|e| OotleWasmError::InvalidPublicKey(e.to_string()))?;
    let view_pk = RistrettoPublicKey::from_canonical_bytes(view_public_key)
        .map_err(|e| OotleWasmError::InvalidPublicKey(e.to_string()))?;

    use ootle_byte_type::ToByteType;
    let mut address = OotleAddress::new(network, view_pk.to_byte_type(), owner_pk.to_byte_type());

    if let Some(memo_bytes) = memo {
        let pay_ref =
            PayRef::new_checked(memo_bytes.to_vec()).ok_or_else(|| OotleWasmError::InvalidPayRef(memo_bytes.len()))?;
        address = address.with_pay_ref(pay_ref);
    }

    Ok(address.to_bech32_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_secret_key_produces_valid_keys() {
        let sk = generate_ootle_secret_key();
        assert_eq!(sk.owner_key.len(), 32);
        assert_eq!(sk.view_key.len(), 32);
    }

    #[test]
    fn generate_secret_key_is_unique() {
        let sk1 = generate_ootle_secret_key();
        let sk2 = generate_ootle_secret_key();
        assert_ne!(sk1.owner_key, sk2.owner_key);
        assert_ne!(sk1.view_key, sk2.view_key);
    }

    #[test]
    fn derive_public_keys_from_secret_keys() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();
        assert_eq!(pk.owner_key.len(), 32);
        assert_eq!(pk.view_key.len(), 32);

        // Deriving again should produce the same result
        let pk2 = ootle_public_key_from_secret_key(&sk).unwrap();
        assert_eq!(pk.owner_key, pk2.owner_key);
        assert_eq!(pk.view_key, pk2.view_key);
    }

    #[test]
    fn generate_address_without_memo() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();

        let address = generate_ootle_address(&pk.owner_key, &pk.view_key, Network::LocalNet as u8, None).unwrap();
        assert!(address.starts_with("otl_loc_"));

        // Should be parseable back
        let parsed: OotleAddress = address.parse().unwrap();
        assert_eq!(parsed.network(), Network::LocalNet);
        assert_eq!(parsed.pay_ref(), None);
    }

    #[test]
    fn generate_address_with_memo() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();

        let memo = b"invoice-12345";
        let address =
            generate_ootle_address(&pk.owner_key, &pk.view_key, Network::LocalNet as u8, Some(memo)).unwrap();
        assert!(address.starts_with("otl_loc_"));

        let parsed: OotleAddress = address.parse().unwrap();
        assert_eq!(parsed.pay_ref().unwrap().as_bytes(), memo);
    }

    #[test]
    fn generate_address_rejects_oversized_memo() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();

        let memo = vec![0u8; PayRef::MAX_LEN + 1];
        let result = generate_ootle_address(&pk.owner_key, &pk.view_key, Network::LocalNet as u8, Some(&memo));
        assert!(result.is_err());
    }

    #[test]
    fn parse_address_round_trip() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();

        let address = generate_ootle_address(&pk.owner_key, &pk.view_key, Network::LocalNet as u8, None).unwrap();
        let parsed = parse_ootle_address(&address).unwrap();

        assert_eq!(parsed.owner_key, pk.owner_key);
        assert_eq!(parsed.view_key, pk.view_key);
        assert_eq!(parsed.network, Network::LocalNet as u8);
        assert_eq!(parsed.memo, None);
    }

    #[test]
    fn parse_address_with_memo_round_trip() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();

        let memo = b"payment-ref-42";
        let address =
            generate_ootle_address(&pk.owner_key, &pk.view_key, Network::Esmeralda as u8, Some(memo)).unwrap();
        let parsed = parse_ootle_address(&address).unwrap();

        assert_eq!(parsed.owner_key, pk.owner_key);
        assert_eq!(parsed.view_key, pk.view_key);
        assert_eq!(parsed.network, Network::Esmeralda as u8);
        assert_eq!(parsed.memo.as_deref(), Some(memo.as_slice()));
    }

    #[test]
    fn parse_invalid_address_fails() {
        assert!(parse_ootle_address("not_a_valid_address").is_err());
    }

    #[test]
    fn generate_address_different_networks() {
        let sk = generate_ootle_secret_key();
        let pk = ootle_public_key_from_secret_key(&sk).unwrap();

        let addr_esm = generate_ootle_address(&pk.owner_key, &pk.view_key, Network::Esmeralda as u8, None).unwrap();
        assert!(addr_esm.starts_with("otl_esm_"));

        let addr_main = generate_ootle_address(&pk.owner_key, &pk.view_key, Network::MainNet as u8, None).unwrap();
        assert!(addr_main.starts_with("otl_"));
        assert!(!addr_main.starts_with("otl_loc_"));
    }
}
