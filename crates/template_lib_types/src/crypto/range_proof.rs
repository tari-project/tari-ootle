//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::rust::fmt;

use crate::serde_helpers;

/// A range proof is a cryptographic proof that a value is within a certain range without revealing the value itself.
/// This struct is used to represent the range proof bytes in a serialized format.
/// The length of the range proof is limited to a maximum size (1024 bytes) to mitigate potential denial-of-service
/// attacks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct RangeProofBytes(
    #[serde(
        serialize_with = "serde_helpers::dynamic_hex::serialize",
        deserialize_with = "serde_validated_range_proof_deserialize_impl"
    )]
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    Vec<u8>,
);

impl RangeProofBytes {
    // TODO: is this sufficiently large? How many outputs can we aggregate before we hit this limit?
    // observed size for 2 commitments is ~500 bytes. For 500 commitments, it is 1153 bytes.
    pub const MAX_LENGTH: usize = 1153;

    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub const fn len(&self) -> usize {
        self.0.len()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<Vec<u8>> for RangeProofBytes {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for RangeProofBytes {
    type Error = RangeProofSizeExceeded;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if bytes.len() > Self::MAX_LENGTH {
            return Err(RangeProofSizeExceeded { size: bytes.len() });
        }
        Ok(Self(bytes))
    }
}

/// Ensures deserialized bytes are a valid size
fn serde_validated_range_proof_deserialize_impl<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where D: serde::Deserializer<'de> {
    let bytes: Vec<u8> = serde_helpers::dynamic_hex::deserialize::<'_, D, _>(deserializer)?;
    if bytes.len() > RangeProofBytes::MAX_LENGTH {
        return Err(serde::de::Error::custom(RangeProofSizeExceeded { size: bytes.len() }));
    }
    Ok(bytes)
}

#[derive(Debug)]
pub struct RangeProofSizeExceeded {
    size: usize,
}

impl fmt::Display for RangeProofSizeExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Range proof size ({} bytes) exceeded the maximum ({} bytes).",
            self.size,
            RangeProofBytes::MAX_LENGTH
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RangeProofSizeExceeded {}
