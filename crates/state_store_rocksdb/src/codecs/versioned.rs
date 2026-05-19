//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::io::Write;

use crate::{
    codecs::{DbDecoder, DbEncoder, EncodeVec},
    error::RocksDbStorageError,
    traits::Versioned,
};

pub struct VersionedCodec<C, T> {
    codec: C,
    _versioned: std::marker::PhantomData<T>,
}

impl<C: DbEncoder<T>, T: Versioned<Latest = V>, V: Into<T> + Clone> DbEncoder<V> for VersionedCodec<C, T> {
    fn encode_len(&self, value: &V) -> Result<usize, RocksDbStorageError> {
        self.codec.encode_len(&value.clone().into())
    }

    fn encode_into<W: Write>(&self, value: &V, writer: &mut W) -> Result<(), RocksDbStorageError> {
        self.codec.encode_into(&value.clone().into(), writer)
    }

    fn encode(&self, value: &V) -> Result<EncodeVec, RocksDbStorageError> {
        let value = value.clone().into();
        self.codec.encode(&value)
    }
}

impl<C: DbDecoder<T>, T: Versioned<Latest = V>, V: Into<T> + Clone> DbDecoder<V> for VersionedCodec<C, T> {
    fn decode(&self, bytes: &[u8]) -> Result<(V, usize), RocksDbStorageError> {
        let (versioned, consumed) = self.codec.decode(bytes)?;
        Ok((versioned.full_upgrade().into_latest(), consumed))
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
    use std::io::Read;

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::codecs::DefaultVersionedCodec;

    #[derive(Serialize, Deserialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
    enum VersionedSomething1 {
        #[n(0)]
        V1(#[n(0)] u32),
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

    #[derive(Serialize, Deserialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
    enum VersionedSomething2 {
        #[n(0)]
        V1(#[n(0)] u32),
        #[n(1)]
        V2(#[n(0)] Vec<u8>),
        #[n(2)]
        V3 {
            #[n(0)]
            s: String,
            #[n(1)]
            decimals: u8,
        },
    }

    impl Versioned for VersionedSomething2 {
        type Latest = (String, u8);

        fn upgrade_single_step(self) -> (Self, bool) {
            match self {
                Self::V1(value) => (Self::V2(value.to_be_bytes().to_vec()), true),
                Self::V2(bytes) => {
                    let mut b = bytes.as_slice();

                    let mut array = [0u8; 4];
                    b.read_exact(&mut array).unwrap();
                    // NB: all upgrades must be infallible. If they are not, then the upgrade was implemented
                    // incorrectly, the database is corrupt and there's nothing further we can do (panic is
                    // appropriate).
                    let n = u32::from_be_bytes(array);
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
        let decoded = codec.decode_exact(&encoded).expect("Decoding should succeed");
        assert_eq!(decoded, 42);

        // Later on we load this data that is two versions behind. We check the previously encoded value upgrades
        // correctly.
        let codec = DefaultVersionedCodec::<VersionedSomething2>::default();
        let decoded = codec.decode_exact(&encoded).expect("Decoding should succeed");
        assert_eq!(decoded, ("42".to_string(), 8));
    }
}
