//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::convert::Infallible;

use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::crypto::ValueLookupTable;
use tari_ootle_common_types::array_utils::copy_fixed_checked;
use tari_utilities::ByteArray;

#[derive(Clone)]
pub struct GenerateValueLookup;

impl ValueLookupTable for GenerateValueLookup {
    type Error = Infallible;

    fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error> {
        let pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(value));
        Ok(Some(
            copy_fixed_checked(pk.as_bytes()).expect("Ristretto public key is always 32 bytes"),
        ))
    }
}
