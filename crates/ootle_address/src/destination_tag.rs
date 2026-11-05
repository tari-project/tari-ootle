//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
pub struct DestinationTag(Box<[u8]>);

impl DestinationTag {
    /// Maximum length of a DestinationTag in bytes. This is chosen to easily fit within typical memo field constraints.
    pub const MAX_LEN: usize = 64;

    pub fn new_checked<T: Into<Box<[u8]>>>(contents: T) -> Option<Self> {
        let contents = contents.into();
        if contents.is_empty() || contents.len() > Self::MAX_LEN {
            None
        } else {
            Some(Self(contents))
        }
    }

    pub const fn len(&self) -> usize {
        self.0.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::new_checked(bytes)
    }
}

impl AsRef<[u8]> for DestinationTag {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<DestinationTag> for Box<[u8]> {
    fn from(destination_tag: DestinationTag) -> Self {
        destination_tag.0
    }
}

#[cfg(feature = "serde")]
mod serde_impl {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use tari_template_lib_types::{
        hex::{bytes_from_hex, bytes_to_hex},
        serde_helpers::BytesVisitor,
    };

    use super::DestinationTag;

    impl Serialize for DestinationTag {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            if serializer.is_human_readable() {
                let hex = bytes_to_hex(&self.0);
                serializer.serialize_str(&hex)
            } else {
                serializer.serialize_bytes(&self.0)
            }
        }
    }

    impl<'de> Deserialize<'de> for DestinationTag {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            if deserializer.is_human_readable() {
                let s = String::deserialize(deserializer)?;
                let bytes = bytes_from_hex(&s).map_err(serde::de::Error::custom)?;
                DestinationTag::new_checked(bytes)
                    .ok_or_else(|| serde::de::Error::custom("Invalid DestinationTag length"))
            } else {
                let bytes = deserializer.deserialize_byte_buf(BytesVisitor::new())?;
                DestinationTag::new_checked(bytes)
                    .ok_or_else(|| serde::de::Error::custom("Invalid DestinationTag length"))
            }
        }
    }
}
