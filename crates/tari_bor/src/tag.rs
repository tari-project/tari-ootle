//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(not(feature = "std"))]
use alloc::format;
use core::ops::{Deref, DerefMut};

pub trait MaybeTagged {
    fn maybe_tag(&self) -> Option<u64>;

    fn is_tag_of<T: Tagged>(&self) -> bool {
        self.maybe_tag() == Some(T::TAG)
    }

    fn tag_matches<T: MaybeTagged>(&self, tagged: &T) -> bool {
        self.maybe_tag().is_some_and(|t| Some(t) == tagged.maybe_tag())
    }
}

pub trait Tagged {
    const TAG: u64;

    fn is_tag_of<T: Tagged>() -> bool {
        T::TAG == Self::TAG
    }
}

impl<T: Tagged> MaybeTagged for T {
    fn maybe_tag(&self) -> Option<u64> {
        Some(T::TAG)
    }
}

impl MaybeTagged for crate::Value {
    fn maybe_tag(&self) -> Option<u64> {
        self.as_tag().map(|(t, _)| t)
    }
}

/// CBOR-tagged value: encodes as `<TAG, inner>` per RFC 8949 §3.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BorTag<T, const TAG: u64>(T);

impl<T, const TAG: u64> Tagged for BorTag<T, TAG> {
    const TAG: u64 = TAG;
}

impl<T, const TAG: u64> BorTag<T, TAG> {
    pub const fn new(t: T) -> Self {
        Self(t)
    }

    pub const fn inner(&self) -> &T {
        &self.0
    }

    pub const fn inner_mut(&mut self) -> &mut T {
        &mut self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<C, T: minicbor::Encode<C>, const TAG: u64> minicbor::Encode<C> for BorTag<T, TAG> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.tag(minicbor::data::Tag::new(TAG))?;
        self.0.encode(e, ctx)?;
        Ok(())
    }
}

impl<'b, C, T: minicbor::Decode<'b, C>, const TAG: u64> minicbor::Decode<'b, C> for BorTag<T, TAG> {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let actual: u64 = d.tag()?.into();
        if actual != TAG {
            return Err(minicbor::decode::Error::message(format!(
                "BorTag<_, {TAG}> expected tag {TAG}, got {actual}"
            )));
        }
        Ok(BorTag(T::decode(d, ctx)?))
    }
}

impl<T, const TAG: u64> Deref for BorTag<T, TAG> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl<T, const TAG: u64> DerefMut for BorTag<T, TAG> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner_mut()
    }
}

impl<T: AsRef<[u8]>, const TAG: u64> AsRef<[u8]> for BorTag<T, TAG> {
    fn as_ref(&self) -> &[u8] {
        self.inner().as_ref()
    }
}

#[cfg(feature = "serde")]
mod serde_impl {
    use serde::{Deserialize, Serialize, de, ser};

    use super::*;

    /// JSON / serde representation is the bare inner value — the CBOR tag is meaningless
    /// outside CBOR. This matches how the JSON-RPC API rendered tagged types previously.
    impl<T: Serialize, const TAG: u64> Serialize for BorTag<T, TAG> {
        #[inline]
        fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            T::serialize(&self.0, serializer)
        }
    }

    impl<'de, T: Deserialize<'de>, const TAG: u64> Deserialize<'de> for BorTag<T, TAG> {
        #[inline]
        fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            Ok(BorTag(T::deserialize(deserializer)?))
        }
    }
}

#[cfg(feature = "borsh")]
mod borsh_impl {
    use super::*;

    impl<T: borsh::BorshSerialize, const TAG: u64> borsh::BorshSerialize for BorTag<T, TAG> {
        fn serialize<W: borsh::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
            borsh::BorshSerialize::serialize(&TAG, writer)?;
            borsh::BorshSerialize::serialize(self.inner(), writer)
        }
    }

    impl<T: borsh::BorshDeserialize, const TAG: u64> borsh::BorshDeserialize for BorTag<T, TAG> {
        fn deserialize_reader<R>(reader: &mut R) -> Result<Self, borsh::io::Error>
        where R: borsh::io::Read {
            let tag = u64::deserialize_reader(reader)?;
            if tag != TAG {
                return Err(borsh::io::Error::new(
                    borsh::io::ErrorKind::InvalidInput,
                    format!("Invalid tag: expected {}, got {}", TAG, tag),
                ));
            }
            let value = borsh::BorshDeserialize::deserialize_reader(reader)?;
            Ok(BorTag::new(value))
        }
    }
}

#[cfg(all(feature = "std", feature = "ts"))]
mod ts_impl {
    use std::path::PathBuf;

    use ts_rs::{TS, TypeVisitor};

    use super::*;

    impl<T: TS, const TAG: u64> TS for BorTag<T, TAG> {
        type OptionInnerType = T::OptionInnerType;
        type WithoutGenerics = T::WithoutGenerics;

        fn name() -> String {
            T::name()
        }

        fn inline() -> String {
            T::inline()
        }

        fn inline_flattened() -> String {
            T::inline_flattened()
        }

        fn decl() -> String {
            T::decl()
        }

        fn decl_concrete() -> String {
            T::decl_concrete()
        }

        fn ident() -> String {
            T::ident()
        }

        fn docs() -> Option<String> {
            T::docs()
        }

        fn visit_dependencies(v: &mut impl TypeVisitor)
        where Self: 'static {
            T::visit_dependencies(v);
        }

        fn visit_generics(v: &mut impl TypeVisitor)
        where Self: 'static {
            T::visit_generics(v);
        }

        fn output_path() -> Option<PathBuf> {
            T::output_path()
        }

        fn default_output_path() -> Option<PathBuf> {
            T::default_output_path()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_exact, encode};

    #[test]
    fn round_trips_through_minicbor() {
        let t = BorTag::<_, 123>::new(222u32);
        let bytes = encode(&t).unwrap();
        let decoded: BorTag<u32, 123> = decode_exact(&bytes).unwrap();
        assert_eq!(decoded, t);
    }

    #[test]
    fn rejects_wrong_tag() {
        let t = BorTag::<_, 123>::new(222u32);
        let bytes = encode(&t).unwrap();
        let err = decode_exact::<BorTag<u32, 999>>(&bytes).unwrap_err();
        assert!(err.into_string().contains("expected tag 999"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_json_is_unwrapped() {
        let t = BorTag::<_, 123>::new(222u8);
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(s, "222");
        let b: BorTag<u8, 123> = serde_json::from_str(&s).unwrap();
        assert_eq!(*b, 222u8);
    }
}
