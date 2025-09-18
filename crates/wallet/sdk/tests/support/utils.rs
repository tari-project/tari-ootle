//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use digest::crypto_common::rand_core::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_template_lib::{models::ResourceAddress, types::ObjectKey};

pub fn random_keypair() -> (RistrettoSecretKey, RistrettoPublicKey) {
    let secret_key = random_key();
    let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
    (secret_key, public_key)
}

pub fn random_key() -> RistrettoSecretKey {
    RistrettoSecretKey::random(&mut OsRng)
}

pub fn resource_address_from_seed(seed: u8) -> ResourceAddress {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    ResourceAddress::new(ObjectKey::from_array([seed; ObjectKey::LENGTH]))
}
