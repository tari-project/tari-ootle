//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use alloc::{format, vec};
use core::fmt::{Display, Formatter};

use curve25519_dalek::Scalar;
use ledger_device_sdk::{
    ecc::{CurvesId, CxError, bip32_derive, make_bip32_path},
    hash::{HashError, HashInit, blake2::Blake2b_512},
};
use ootle_ledger_common::{OotleStatusWord, arg_types::KeyType};
use zeroize::Zeroizing;

use crate::{constants::BIP32_COIN_TYPE, status::AppStatus};

/// Derive a secret key from a BIP32 path. In case of an error, display an interactive message on the device.
pub fn derive_from_bip32_key(account: u64, index: u64, key_type: KeyType) -> Result<Scalar, AppStatus> {
    let bip32_path = format!("m/44'/{BIP32_COIN_TYPE}'/{account}'/0/{index}'/{}'", key_type.as_u64());
    let path: [u32; 6] = make_bip32_path(bip32_path.as_bytes());

    match get_raw_key_hash(&path) {
        Ok(val) => Ok(Scalar::from_bytes_mod_order_wide(&val)),
        Err(e) => Err(AppStatus::OotleStatusWithMessages {
            messages: vec!["Err: raw key >>".into(), format!("{:?}", e).into()],
            status: OotleStatusWord::KeyDeriveFail,
        }),
    }
}

/// Get a raw 64 byte key hash from the BIP32 path.
/// Note: We use `CurvesId::Secp256k1` as the curve for the bip32 key derivation because it provides better entropy when
///       compared to `CurvesId::Ed25519`. There is also no need for compatibility to `tari_crypto` as the output is
/// only       ever used in a subsequent key derivation function.
fn get_raw_bip32_key(path: &[u32]) -> Result<Zeroizing<[u8; 64]>, CxError> {
    let mut key_buffer = Zeroizing::new([0u8; 64]);
    bip32_derive(CurvesId::Secp256k1, path, key_buffer.as_mut(), None)?;
    Ok(key_buffer)
}

///  This function applies domain separated hashing to the 64 byte private key of the returned buffer to get 64
///  uniformly distributed random bytes.
fn get_raw_key_hash(path: &[u32]) -> Result<Zeroizing<[u8; 64]>, DeriveError> {
    let raw_key_64 = get_raw_bip32_key(path)?;

    let mut raw_key_hashed = Zeroizing::new([0u8; 64]);
    let mut hasher = Blake2b_512::new();
    hasher.update(b"tari_ootle_raw_key")?;
    hasher.update(raw_key_64.as_ref())?;
    hasher.finalize(raw_key_hashed.as_mut())?;
    Ok(raw_key_hashed)
}

#[derive(Debug)]
pub enum DeriveError {
    CxError(CxError),
    HashError(HashError),
}

impl From<CxError> for DeriveError {
    fn from(e: CxError) -> Self {
        Self::CxError(e)
    }
}

impl From<HashError> for DeriveError {
    fn from(e: HashError) -> Self {
        Self::HashError(e)
    }
}

impl Display for DeriveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            DeriveError::CxError(e) => write!(f, "CxError: {:?}", e),
            DeriveError::HashError(e) => write!(f, "HashError: {:?}", e),
        }
    }
}
