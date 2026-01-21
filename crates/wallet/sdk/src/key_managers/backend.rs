//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_common_types::signature::SignatureOutput;
use tari_ootle_transaction::Signable;

use crate::models::DerivedKeyIndex;

pub trait WalletKeyStore {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Derive a secret key for the given branch and key index.
    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error>;

    /// Sign a message using a derived key for the given branch and key index.
    fn sign<M: Signable<C>, C>(
        &self,
        key_branch: &str,
        index: DerivedKeyIndex,
        context: C,
        message: &M,
    ) -> Result<SignatureOutput, Self::Error> {
        let secret = self.derive_secret(key_branch, index)?;
        let signature = RistrettoSchnorr::sign(&secret, message.to_signing_message(context), &mut OsRng)
            .expect("RistrettoSchnorr::sign is infallible as it internally hashes the message into canonical form");
        Ok(SignatureOutput {
            signature,
            public_key: RistrettoPublicKey::from_secret_key(&secret),
        })
    }

    /// Retrieve the key birthday if it exists. The birthday is defined as the number of seconds since the zero epoch,
    /// which is predefined for a given network. If this is not supported, it is correct to return Ok(None).
    fn key_birthday(&self) -> Result<Option<u16>, Self::Error>;
}

pub trait WithCipherSeed {
    type Error: std::error::Error + Send + Sync + 'static;
    fn get_cipher_seed(&self) -> Result<&CipherSeed, Self::Error>;
}

impl WithCipherSeed for CipherSeed {
    type Error = std::convert::Infallible;

    fn get_cipher_seed(&self) -> Result<&CipherSeed, Self::Error> {
        Ok(self)
    }
}

impl<T: WithCipherSeed> WalletKeyStore for T {
    type Error = T::Error;

    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error> {
        let cipher_seed = self.get_cipher_seed()?;
        let secret = helpers::derive_private_key(cipher_seed.entropy(), branch.as_bytes(), key_index);
        Ok(secret)
    }

    fn key_birthday(&self) -> Result<Option<u16>, Self::Error> {
        let seed = self.get_cipher_seed()?;
        Ok(Some(seed.birthday()))
    }
}

mod helpers {
    use tari_crypto::{hashing::DomainSeparatedHasher, keys::SecretKey, ristretto::RistrettoSecretKey};

    pub fn derive_private_key(entropy: &[u8], branch_seed: &[u8], account: u64) -> RistrettoSecretKey {
        use blake2::{digest::consts::U64, Blake2b};
        use digest::typenum::ToInt;
        use tari_hashing::KeyManagerDomain;

        pub const HASHER_LABEL_DERIVE_KEY: &str = "derive_key";
        const fn assert_equal(a: usize, b: usize) {
            if a != b {
                panic!("RistrettoSecretKey::WIDE_REDUCTION_LEN is not equal to 64");
            }
        }

        let derive_key =
            DomainSeparatedHasher::<Blake2b<U64>, KeyManagerDomain>::new_with_label(HASHER_LABEL_DERIVE_KEY)
                .chain(entropy)
                .chain(branch_seed)
                .chain(account.to_le_bytes())
                .finalize();

        // At compile time, fail if the length of the derived key is not equal to the expected length which would lead
        // to a runtime panic
        const _: () = assert_equal(RistrettoSecretKey::WIDE_REDUCTION_LEN, U64::INT);

        RistrettoSecretKey::from_uniform_bytes(derive_key.as_ref())
            .expect("derived key length matches RistrettoSecretKey length")
    }
}
