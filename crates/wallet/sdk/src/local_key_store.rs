//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::{seeds::cipher_seed::CipherSeed, types::PrivateKey};
use tari_crypto::{hashing::DomainSeparatedHasher, keys::SecretKey, ristretto::RistrettoSecretKey};
use tari_ootle_wallet_crypto::encryption::CipherError;

use crate::{
    apis::password_manager::PasswordManagerApiError,
    cipher_seed::{SafeCipherSeed, WalletCipherSeed},
    key_managers::WalletKeyStore,
    models::DerivedKeyIndex,
    storage::WalletStorageError,
};

#[derive(Clone)]
pub struct LocalKeyStore {
    cipher_seed: WalletCipherSeed,
}

impl LocalKeyStore {
    pub fn new(cipher_seed: WalletCipherSeed) -> Self {
        Self { cipher_seed }
    }

    pub fn set_cipher_seed(&mut self, cipher_seed: SafeCipherSeed) -> &mut Self {
        self.cipher_seed = WalletCipherSeed::CipherSeed(cipher_seed);
        self
    }

    pub fn cipher_seed(&self) -> Option<&SafeCipherSeed> {
        self.cipher_seed.cipher_seed()
    }

    fn get_cipher_seed(&self) -> Result<&SafeCipherSeed, LocalKeyStoreError> {
        self.cipher_seed().ok_or(LocalKeyStoreError::NoCipherSeed)
    }
}

impl WalletKeyStore for LocalKeyStore {
    type Error = LocalKeyStoreError;

    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error> {
        let cipher_seed = self.get_cipher_seed()?;
        let secret = derive_private_key(cipher_seed, branch.to_string(), key_index);
        Ok(secret)
    }

    fn key_birthday(&self) -> Result<Option<u16>, Self::Error> {
        let seed = self.get_cipher_seed()?;
        Ok(Some(seed.birthday()))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LocalKeyStoreError {
    #[error("Password manager error: {0}")]
    PasswordManager(#[from] PasswordManagerApiError),
    #[error("Wallet storage error: {0}")]
    WalletStorage(#[from] WalletStorageError),
    #[error("Cipher error: {0}")]
    Cipher(#[from] CipherError),
    #[error("Cannot derive keys because no cipher seed was provided")]
    NoCipherSeed,
}

fn derive_private_key(seed: &CipherSeed, branch_seed: String, account: u64) -> PrivateKey {
    use blake2::{digest::consts::U64, Blake2b};
    use digest::typenum::ToInt;
    use tari_hashing::KeyManagerDomain;

    pub const HASHER_LABEL_DERIVE_KEY: &str = "derive_key";
    const fn assert_equal(a: usize, b: usize) {
        if a != b {
            panic!("RistrettoSecretKey::WIDE_REDUCTION_LEN is not equal to 64");
        }
    }

    let derive_key = DomainSeparatedHasher::<Blake2b<U64>, KeyManagerDomain>::new_with_label(HASHER_LABEL_DERIVE_KEY)
        .chain(seed.entropy())
        .chain(branch_seed.as_bytes())
        .chain(account.to_le_bytes())
        .finalize();

    // At compile time, fail if the length of the derived key is not equal to the expected length which would lead to a
    // runtime panic
    const _: () = assert_equal(RistrettoSecretKey::WIDE_REDUCTION_LEN, U64::INT);

    PrivateKey::from_uniform_bytes(derive_key.as_ref()).expect("derived key length matches RistrettoSecretKey length")
}
