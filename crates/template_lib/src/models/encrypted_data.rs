//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_template_lib_types::serde_helpers;

/// Used by the receiver to determine the value and mask of the commitment. Used in stealth and confidential transfers,
/// as well as Minotari burns
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct EncryptedData(
    #[serde(with = "serde_helpers::dynamic_hex")]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    Box<[u8]>,
);

impl EncryptedData {
    pub const ENCRYPTED_DATA_SIZE_TOTAL: usize = Self::SIZE_NONCE + Self::SIZE_VALUE + Self::SIZE_MASK + Self::SIZE_TAG;
    pub const SIZE_MASK: usize = 32;
    pub const SIZE_NONCE: usize = 24;
    pub const SIZE_TAG: usize = 16;
    pub const SIZE_VALUE: usize = size_of::<u64>();

    pub const fn min_size() -> usize {
        Self::ENCRYPTED_DATA_SIZE_TOTAL
    }

    pub const fn max_size() -> usize {
        Self::min_size() + 256
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn tag_slice(&self) -> Option<&[u8]> {
        self.0.get(..Self::SIZE_TAG)
    }

    pub fn nonce_slice(&self) -> Option<&[u8]> {
        self.0.get(Self::SIZE_TAG..Self::SIZE_NONCE + Self::SIZE_TAG)
    }

    pub fn payload_slice(&self) -> Option<&[u8]> {
        self.0.get(Self::payload_offset()..)
    }

    pub const fn payload_offset() -> usize {
        Self::SIZE_TAG + Self::SIZE_NONCE
    }
}

impl AsRef<[u8]> for EncryptedData {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for EncryptedData {
    type Error = usize;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        if value.len() < Self::min_size() {
            return Err(value.len());
        }
        if value.len() > Self::max_size() {
            return Err(value.len());
        }
        Ok(Self(value.into_boxed_slice()))
    }
}
