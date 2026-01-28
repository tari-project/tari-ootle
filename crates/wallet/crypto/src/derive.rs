//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{hashing::DomainSeparatedHasher, keys::SecretKey, ristretto::RistrettoSecretKey};

pub fn derive_ristretto_key(entropy: &[u8], branch: &[u8], account: u64) -> RistrettoSecretKey {
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
        .chain(entropy)
        .chain(branch)
        .chain(account.to_le_bytes())
        .finalize();

    // At compile time, fail if the length of the derived key is not equal to the expected length which would lead
    // to a runtime panic
    const _: () = assert_equal(RistrettoSecretKey::WIDE_REDUCTION_LEN, U64::INT);

    RistrettoSecretKey::from_uniform_bytes(derive_key.as_ref())
        .expect("derived key length matches RistrettoSecretKey length")
}
