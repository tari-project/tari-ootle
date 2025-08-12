//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use chacha20poly1305::{aead::generic_array::GenericArray, Key};
use digest::FixedOutput;
use tari_crypto::{
    dhke::DiffieHellmanSharedSecret,
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::{base_layer_hashing::encrypted_data_hasher, Network};
use tari_template_lib::{prelude::RistrettoPublicKeyBytes, types::crypto::UtxoTagByte};
use tari_utilities::{hidden_type, safe_array::SafeArray, Hidden};
use zeroize::Zeroize;

use crate::hashers::stealth_owner_hasher64;

pub(crate) const AEAD_KEY_LEN: usize = size_of::<Key>();

// Type for hiding aead key encryption
hidden_type!(EncryptedDataKey, SafeArray<u8, AEAD_KEY_LEN>);
hidden_type!(SafeKey64, SafeArray<u8, 64>);

/// Generate a decryption key from a private key and nonce
pub fn encrypted_data_dh_kdf_aead(
    private_key: &RistrettoSecretKey,
    public_nonce: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    let shared_secret = DiffieHellmanSharedSecret::<RistrettoPublicKey>::new(private_key, public_nonce);
    let mut aead_key = SafeKey64::from(SafeArray::default());
    // Must match base layer burn
    encrypted_data_hasher()
        .chain(shared_secret.as_bytes())
        .finalize_into(GenericArray::from_mut_slice(aead_key.reveal_mut()));

    RistrettoSecretKey::from_uniform_bytes(aead_key.reveal()).unwrap()
}

/// Generate a decryption key for the owner key from a private key and nonce
pub fn owner_stealth_dh_secret(
    network: Network,
    private_key: &RistrettoSecretKey,
    public_nonce: &RistrettoPublicKey,
) -> RistrettoSecretKey {
    // c = H(k * r.G)
    let shared_secret = DiffieHellmanSharedSecret::<RistrettoPublicKey>::new(private_key, public_nonce);
    let result = stealth_owner_hasher64(network)
        .chain(shared_secret.as_bytes())
        .finalize();

    let c = RistrettoSecretKey::from_uniform_bytes(result.as_ref())
        .expect("key length != RistrettoSecretKey::WIDE_REDUCTION_LEN");

    // c + k
    c + private_key
}
pub fn owner_stealth_dh_stealth_address(
    network: Network,
    public_key: &RistrettoPublicKey,
    secret_nonce: &RistrettoSecretKey,
) -> RistrettoPublicKey {
    // c = H(r * k.G)
    let shared_secret = DiffieHellmanSharedSecret::<RistrettoPublicKey>::new(secret_nonce, public_key);
    let result = stealth_owner_hasher64(network)
        .chain(shared_secret.as_bytes())
        .finalize();

    let c = RistrettoSecretKey::from_uniform_bytes(result.as_ref())
        .expect("key length != RistrettoSecretKey::WIDE_REDUCTION_LEN");

    let c_g = RistrettoPublicKey::from_secret_key(&c);

    // c.G + k.G
    c_g + public_key
}

pub fn derive_stealth_output_tag(network: Network, owner_public_key: &RistrettoPublicKeyBytes) -> UtxoTagByte {
    let result = stealth_owner_hasher64(network).chain(owner_public_key).finalize();

    UtxoTagByte::new(result[0])
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::crypto::create_key_pair_from_seed;

    use super::*;

    #[test]
    fn it_generates_the_correct_private_stealth_address() {
        let network = Network::LocalNet;
        let (secret_key, public_key) = create_key_pair_from_seed(123);
        let (secret_nonce, public_nonce) = create_key_pair_from_seed(234);

        let stealth_address = owner_stealth_dh_stealth_address(network, &public_key, &secret_nonce);
        let stealth_secret = owner_stealth_dh_secret(network, &secret_key, &public_nonce);
        let expected_stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
        assert_eq!(stealth_address, expected_stealth_address);
    }
}
