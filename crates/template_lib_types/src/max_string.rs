//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use tari_template_abi::rust::{
    fmt,
    fmt::Display,
    format,
    ops::{Deref, DerefMut},
    prelude::*,
};

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize, borsh::BorshDeserialize))]
pub struct MaxString<const N: usize>(Box<str>);

impl<const N: usize> Deref for MaxString<N> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> MaxString<N> {
    pub fn new_checked(s: impl Into<Box<str>>) -> Option<Self> {
        let s = s.into();
        if s.len() <= N { Some(Self(s)) } else { None }
    }

    /// Creates a new `MaxString` without checking the length.
    ///
    /// # Safety
    /// The caller must ensure that the length of the string does not exceed `N`.
    pub unsafe fn new_unchecked(s: impl Into<Box<str>>) -> Self {
        let s = s.into();
        debug_assert!(s.len() <= N, "string length exceeds maximum of {}: got {}", N, s.len());
        Self(s)
    }

    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

impl<const N: usize> AsRef<str> for MaxString<N> {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<const N: usize> DerefMut for MaxString<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Mutable but not resizeable
        &mut self.0
    }
}

impl<const N: usize> Default for MaxString<N> {
    fn default() -> Self {
        Self("".into())
    }
}

#[cfg(feature = "serde")]
impl<const N: usize> serde::Serialize for MaxString<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de, const N: usize> serde::Deserialize<'de> for MaxString<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        let len = s.len();
        Self::new_checked(s)
            .ok_or_else(|| serde::de::Error::custom(format!("string length exceeds maximum of {}: got {}", N, len)))
    }
}

impl<C, const N: usize> minicbor::Encode<C> for MaxString<N> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.str(&self.0)?;
        Ok(())
    }
}

impl<'b, C, const N: usize> minicbor::Decode<'b, C> for MaxString<N> {
    fn decode(d: &mut minicbor::Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let s = d.str()?;
        if s.len() > N {
            return Err(minicbor::decode::Error::message(format!(
                "string length exceeds maximum of {}: got {}",
                N,
                s.len()
            )));
        }
        Ok(MaxString::new_checked(Box::<str>::from(s)).expect("length checked above"))
    }
}

impl<C, const N: usize> minicbor::CborLen<C> for MaxString<N> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        <str as minicbor::CborLen<C>>::cbor_len(&self.0, ctx)
    }
}

impl<const N: usize> TryFrom<String> for MaxString<N> {
    type Error = MaxStringError<N>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(MaxStringError)
    }
}

impl<const N: usize> TryFrom<&String> for MaxString<N> {
    type Error = MaxStringError<N>;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Self::new_checked(value.as_str()).ok_or(MaxStringError)
    }
}

impl<const N: usize> TryFrom<&str> for MaxString<N> {
    type Error = MaxStringError<N>;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(MaxStringError)
    }
}

impl<const N: usize> TryFrom<Box<str>> for MaxString<N> {
    type Error = MaxStringError<N>;

    fn try_from(value: Box<str>) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(MaxStringError)
    }
}

impl<const N: usize> Display for MaxString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.as_ref().fmt(f)
    }
}

pub struct MaxStringError<const N: usize>;

impl<const N: usize> Display for MaxStringError<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "string length exceeds maximum of {}", N)
    }
}

#[cfg(feature = "std")]
mod std_impl {
    use std::fmt;

    use super::*;

    impl<const N: usize> fmt::Debug for MaxStringError<N> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "MaxStringError<{}>", N)
        }
    }
    impl<const N: usize> std::error::Error for MaxStringError<N> {}
}

#[cfg(test)]
mod tests {
    use super::*;

    mod new_checked {
        use super::*;

        #[test]
        fn it_returns_some_if_data_le_size() {
            let b = "123";
            let mb = MaxString::<5>::new_checked(b).unwrap();
            assert_eq!(mb.len(), 3);
            assert_eq!(&mb[..], "123");
        }

        #[test]
        fn it_returns_none_if_data_gt_size() {
            let b = "123456";
            let mb = MaxString::<5>::new_checked(b);
            assert!(mb.is_none());
        }
    }

    #[cfg(feature = "serde")]
    mod serde_impl {
        use super::*;

        #[test]
        fn it_serializes_and_deserializes_as_string() {
            let original = MaxString::<5>::new_checked("12345").unwrap();
            let serialized = tari_bor::encode(&original).unwrap();
            // Now decode it back
            let deserialized: MaxString<5> = tari_bor::decode(&serialized).unwrap();
            assert_eq!(original, deserialized);
        }

        #[test]
        fn it_fails_to_deserialize_if_length_is_too_large() {
            let json = "\"123456\""; // 6 chars, max is 5
            let err: serde_json::Error = serde_json::from_str::<MaxString<5>>(json).unwrap_err();
            assert!(err.to_string().contains("string length exceeds maximum"));

            let bytes = MaxString::<5>(Box::from("123456"));
            let serialized = tari_bor::encode(&bytes).unwrap();
            let err = tari_bor::decode::<MaxString<5>>(&serialized).unwrap_err();
            assert!(err.to_string().contains("string length exceeds maximum"));
        }
    }
}
