//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use crate::{
    codecs::{DbCodec, EncodeVec},
    error::RocksDbStorageError,
    traits::Versioned,
};

pub struct VersionedCodec<C, T> {
    codec: C,
    _versioned: std::marker::PhantomData<T>,
}

impl<C: DbCodec<T>, T: Versioned<Latest = V>, V: Into<T> + Clone> DbCodec<V> for VersionedCodec<C, T> {
    fn encode(&self, value: &V) -> Result<EncodeVec, RocksDbStorageError> {
        let value = value.clone().into();
        self.codec.encode(&value)
    }

    fn decode_reader<R: Read>(&self, reader: &mut R) -> Result<V, RocksDbStorageError> {
        let versioned = self.codec.decode_reader(reader)?;
        Ok(versioned.full_upgrade().into_latest())
    }
}

impl<C: Default, T> Default for VersionedCodec<C, T> {
    fn default() -> Self {
        Self {
            codec: C::default(),
            _versioned: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{codecs::DefaultVersionedCodec, utils::read_to_fixed};

    #[derive(Serialize, Deserialize)]
    enum VersionedSomething1 {
        V1(u32),
    }

    impl Versioned for VersionedSomething1 {
        type Latest = u32;

        fn upgrade_single_step(self) -> (Self, bool) {
            (self, false)
        }

        fn into_latest(self) -> Self::Latest {
            match self {
                Self::V1(value) => value,
            }
        }
    }

    impl From<u32> for VersionedSomething1 {
        fn from(value: u32) -> Self {
            Self::V1(value)
        }
    }

    #[derive(Serialize, Deserialize)]
    enum VersionedSomething2 {
        V1(u32),
        V2(Vec<u8>),
        V3 { s: String, decimals: u8 },
    }

    impl Versioned for VersionedSomething2 {
        type Latest = (String, u8);

        fn upgrade_single_step(self) -> (Self, bool) {
            match self {
                Self::V1(value) => (Self::V2(value.to_be_bytes().to_vec()), true),
                Self::V2(bytes) => {
                    let mut b = bytes.as_slice();
                    // NB: all upgrades must be infallible. If they are not, then the upgrade was implemented
                    // incorrectly, the database is corrupt and there's nothing further we can do (panic is
                    // appropriate).
                    let n = u32::from_be_bytes(read_to_fixed(&mut b).unwrap());
                    (
                        Self::V3 {
                            s: n.to_string(),
                            // Default to 8 decimals - None is also useful in many cases.
                            decimals: 8,
                        },
                        true,
                    )
                },
                Self::V3 { .. } => (self, false),
            }
        }

        fn into_latest(self) -> Self::Latest {
            match self {
                Self::V1(_) | Self::V2(_) => {
                    panic!("VersionedSomething2 was not upgraded to latest before calling into_latest")
                },
                Self::V3 { s, decimals } => (s, decimals),
            }
        }
    }

    impl From<(String, u8)> for VersionedSomething2 {
        fn from((s, decimals): (String, u8)) -> Self {
            Self::V3 { s, decimals }
        }
    }

    #[test]
    fn default_codec_allows_updates_to_versioned_type() {
        // In this test, we check the versioned codec as well as that the default codec can decode a type that has had
        // additional enum variants added later.

        let codec = DefaultVersionedCodec::<VersionedSomething1>::default();
        let encoded = codec.encode(&42).expect("Encoding should succeed");
        let decoded = codec.decode(&encoded).expect("Decoding should succeed");
        assert_eq!(decoded, 42);

        // Later on we load this data that is two versions behind. We check the previously encoded value upgrades
        // correctly.
        let codec = DefaultVersionedCodec::<VersionedSomething2>::default();
        let decoded = codec.decode(&encoded).expect("Decoding should succeed");
        assert_eq!(decoded, ("42".to_string(), 8));
    }
}
