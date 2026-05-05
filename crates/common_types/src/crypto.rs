//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{
    keys::{PublicKey as PublicKeyT, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};

pub fn create_key_pair() -> (RistrettoSecretKey, RistrettoPublicKey) {
    RistrettoPublicKey::random_keypair(&mut rand::rng())
}

pub fn create_key_pair_from_seed(seed: u8) -> (RistrettoSecretKey, RistrettoPublicKey) {
    let private = RistrettoSecretKey::from_uniform_bytes(&[seed; 64]).unwrap();
    let public = RistrettoPublicKey::from_secret_key(&private);
    (private, public)
}
