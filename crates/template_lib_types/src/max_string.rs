//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{
    fmt,
    fmt::Display,
    ops::{Deref, DerefMut},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaxString<const N: usize> {
    s: Box<str>,
}

impl<const N: usize> Deref for MaxString<N> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.s
    }
}

impl<const N: usize> MaxString<N> {
    pub fn new_checked(s: impl Into<Box<str>>) -> Option<Self> {
        let s = s.into();
        if s.len() <= N {
            Some(Self { s })
        } else {
            None
        }
    }

    pub fn into_string(self) -> String {
        self.s.into_string()
    }
}

impl<const N: usize> AsRef<str> for MaxString<N> {
    fn as_ref(&self) -> &str {
        &self.s
    }
}

impl<const N: usize> DerefMut for MaxString<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Mutable but not resizeable
        &mut self.s
    }
}

impl<const N: usize> serde::Serialize for MaxString<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_str(&self.s)
    }
}

impl<'de, const N: usize> serde::Deserialize<'de> for MaxString<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        let len = s.len();
        Self::new_checked(s)
            .ok_or_else(|| serde::de::Error::custom(format!("string length exceeds maximum of {}: got {}", N, len)))
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
        self.s.as_ref().fmt(f)
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

#[cfg(feature = "borsh")]
mod borsh_impl {
    use borsh::io::{Error, ErrorKind, Read, Result, Write};

    use super::*;

    impl<const N: usize> borsh::BorshSerialize for MaxString<N> {
        fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
            self.s.serialize(writer)
        }
    }

    impl<const N: usize> borsh::BorshDeserialize for MaxString<N> {
        fn deserialize_reader<R: Read>(reader: &mut R) -> Result<Self> {
            let s = Box::<str>::deserialize_reader(reader)?;
            if s.len() <= N {
                Ok(Self { s })
            } else {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("string length exceeds maximum of {}: got {}", N, s.len()),
                ))
            }
        }
    }
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

            let bytes = MaxString::<5> { s: "123456".into() };
            let serialized = tari_bor::encode(&bytes).unwrap();
            let err = tari_bor::decode::<MaxString<5>>(&serialized).unwrap_err();
            assert!(err.to_string().contains("string length exceeds maximum"));
        }
    }
}
