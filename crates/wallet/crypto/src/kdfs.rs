//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::Key;
use tari_crypto::{
    dhke::DiffieHellmanSharedSecret,
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::{base_layer_hashing::encrypted_data_hasher, Network};
use tari_template_lib::{models::ResourceAddress, types::crypto::UtxoTag};
use tari_utilities::{hidden_type, safe_array::SafeArray, Hidden};
use zeroize::Zeroize;

use crate::hashers::{stealth_output_tag_hasher64, stealth_owner_hasher64};

pub(crate) const AEAD_KEY_LEN: usize = size_of::<Key>();

// Type for hiding aead key encryption
hidden_type!(EncryptedDataKey, SafeArray<u8, AEAD_KEY_LEN>);
hidden_type!(SafeKey64, SafeArray<u8, 64>);

fn dh(
    public_key: &RistrettoPublicKey,
    private_key: &RistrettoSecretKey,
) -> DiffieHellmanSharedSecret<RistrettoPublicKey> {
    DiffieHellmanSharedSecret::<RistrettoPublicKey>::new(private_key, public_key)
}

/// Generate a decryption key from a private key and nonce
pub fn encrypted_data_dh_kdf_aead(
    private_key: &RistrettoSecretKey,
    public_key: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    let shared_secret = dh(public_key, private_key);

    RistrettoSecretKey::from_uniform_bytes(
        // Must match base layer burn
        encrypted_data_hasher()
            .chain(shared_secret.as_bytes())
            .finalize()
            .as_ref(),
    )
    .unwrap()
}

/// Generate a decryption key for the owner key from a private key and nonce
pub fn owner_stealth_dh_secret(
    network: Network,
    private_key: &RistrettoSecretKey,
    public_nonce: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    // c = H(r.G * k)
    let c = stealth_owner_dh(network, public_nonce, private_key);
    // c + k
    c + private_key
}

fn stealth_owner_dh(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
) -> RistrettoSecretKey {
    let shared_secret = dh(public_key, secret_nonce);
    let result = stealth_owner_hasher64(network)
        .chain(shared_secret.as_bytes())
        .finalize();

    RistrettoSecretKey::from_uniform_bytes(result.as_ref())
        .expect("key length != RistrettoSecretKey::WIDE_REDUCTION_LEN")
}

pub fn owner_stealth_dh_stealth_address(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
) -> RistrettoPublicKey {
    // c = H(k.G * r)
    let c = stealth_owner_dh(network, public_key, secret_nonce);
    // C = c.G
    let c_g = RistrettoPublicKey::from_secret_key(&c);
    // c.G + k.G
    c_g + public_key
}

pub fn utxo_tag_stealth_dh(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
    resource_address: &ResourceAddress,
) -> UtxoTag {
    let shared_secret = dh(public_key, secret_nonce);
    let result = stealth_output_tag_hasher64(network)
        .chain(shared_secret.as_bytes())
        .chain(resource_address)
        .finalize();

    let mut buf = [0u8; size_of::<u32>()];
    buf.copy_from_slice(&result[..size_of::<u32>()]);
    let tag = u32::from_le_bytes(buf);
    UtxoTag::new(tag)
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::crypto::create_key_pair;

    use super::*;

    #[test]
    fn it_generates_the_correct_private_stealth_address() {
        let network = Network::LocalNet;
        let (secret_key, public_key) = create_key_pair();
        let (secret_nonce, public_nonce) = create_key_pair();

        let stealth_address = owner_stealth_dh_stealth_address(network, &public_key, &secret_nonce);
        let stealth_secret = owner_stealth_dh_secret(network, &secret_key, &public_nonce);
        let expected_stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
        assert_eq!(stealth_address, expected_stealth_address);
    }

    #[test]
    fn it_does_not_produce_the_same_secret_when_switching_params() {
        let network = Network::LocalNet;
        let (secret_key, public_key) = create_key_pair();
        let (secret_nonce, public_nonce) = create_key_pair();

        let stealth_address1 = owner_stealth_dh_stealth_address(network, &public_key, &secret_nonce);
        let stealth_address2 = owner_stealth_dh_stealth_address(network, &public_nonce, &secret_key);

        // c + k.G != c + r.G
        // Just makes this fact clear if it isn't obvious
        assert_ne!(stealth_address1, stealth_address2);
    }
}
