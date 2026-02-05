//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{
    format,
    ops::{Deref, DerefMut},
    prelude::*,
    vec,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
#[serde(transparent)]
pub struct MaxVec<const N: usize, T> {
    elems: Box<[T]>,
}

impl<const N: usize, T> Deref for MaxVec<N, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.elems
    }
}

impl<const N: usize, T> MaxVec<N, T> {
    pub fn new_checked(elems: impl Into<Box<[T]>>) -> Option<Self> {
        let elems = elems.into();
        if elems.len() <= N { Some(Self { elems }) } else { None }
    }

    /// Constructs a new `MaxVec<N>` without checking the length of the input.
    /// This is the only way to break the invariant guarantees of `MaxVec<N>`.
    /// NOTE: this exists for testing purposes and should not be used in general.
    ///
    /// # Safety
    /// The caller must ensure that the length of `elems` is less than or equal to `N`.
    pub unsafe fn new_unchecked(elems: impl Into<Box<[T]>>) -> Self {
        Self { elems: elems.into() }
    }

    pub fn into_elems(self) -> Box<[T]> {
        self.elems
    }

    pub fn into_vec(self) -> Vec<T> {
        self.into_elems().into_vec()
    }

    pub fn empty() -> Self {
        Self { elems: Box::new([]) }
    }

    pub fn as_slice(&self) -> &[T] {
        &self.elems
    }
}

impl<const N: usize, T> AsRef<[T]> for MaxVec<N, T> {
    fn as_ref(&self) -> &[T] {
        &self.elems
    }
}

impl<const N: usize, T> DerefMut for MaxVec<N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Mutable but not resizeable
        &mut self.elems
    }
}

impl<const N: usize, T> Default for MaxVec<N, T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const N: usize, T> TryFrom<Vec<T>> for MaxVec<N, T> {
    type Error = ();

    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(())
    }
}

impl<const N: usize, T> TryFrom<Box<[T]>> for MaxVec<N, T> {
    type Error = ();

    fn try_from(value: Box<[T]>) -> Result<Self, Self::Error> {
        Self::new_checked(value).ok_or(())
    }
}

impl<const N: usize, T> From<MaxVec<N, T>> for Vec<T> {
    fn from(value: MaxVec<N, T>) -> Self {
        value.into_vec()
    }
}

impl<'de, const N: usize, T: serde::Deserialize<'de>> serde::Deserialize<'de> for MaxVec<N, T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        let elems: Vec<T> = serde::Deserialize::deserialize(deserializer)?;
        let len = elems.len();
        Self::new_checked(elems)
            .ok_or_else(|| serde::de::Error::custom(format!("sequence length exceeds maximum of {}: got {}", N, len)))
    }
}

impl<const N: usize, T> IntoIterator for MaxVec<N, T> {
    type IntoIter = vec::IntoIter<T>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<const N: usize, T> FromIterator<T> for MaxVec<N, T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let elems: Vec<T> = iter.into_iter().collect();
        Self::new_checked(elems).expect("collected iterator exceeds maximum length")
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    mod new_checked {
        use super::*;

        #[test]
        fn it_returns_some_if_data_le_size() {
            let b = vec![1, 2, 3];
            let mb = MaxVec::<5, _>::new_checked(b).unwrap();
            assert_eq!(mb.len(), 3);
            assert_eq!(&mb[..], &[1, 2, 3]);
        }

        #[test]
        fn it_returns_none_if_data_gt_size() {
            let b = vec![1; 6];
            let mb = MaxVec::<5, _>::new_checked(b);
            assert!(mb.is_none());
        }
    }

    mod serde_impl {
        use tari_bor::Value;

        use super::*;

        #[test]
        fn it_serializes_and_deserializes() {
            let original = MaxVec::<5, _>::new_checked(vec![1, 2, 3, 4, 5]).unwrap();
            let serialized = tari_bor::encode(&original).unwrap();
            // Assert that it encodes to a BOR bytes value using the Bytes variant
            let val: Value = tari_bor::decode(&serialized).unwrap();
            assert_eq!(
                val,
                Value::Array(vec![
                    Value::Integer(1.into()),
                    Value::Integer(2.into()),
                    Value::Integer(3.into()),
                    Value::Integer(4.into()),
                    Value::Integer(5.into()),
                ])
            );
            // Now decode it back
            let deserialized: MaxVec<5, _> = tari_bor::decode(&serialized).unwrap();
            assert_eq!(original, deserialized);
        }

        #[test]
        fn it_fails_to_deserialize_if_length_is_too_large() {
            let json = "[1,2,3,4,5,6]"; // 6 elems, max is 5
            let err: serde_json::Error = serde_json::from_str::<MaxVec<5, u8>>(json).unwrap_err();
            assert!(err.to_string().contains("sequence length exceeds maximum"));

            let bytes = MaxVec::<5, u8> {
                elems: vec![1; 6].into_boxed_slice(),
            };
            let serialized = tari_bor::encode(&bytes).unwrap();
            let err = tari_bor::decode::<MaxVec<5, u8>>(&serialized).unwrap_err();
            assert!(err.to_string().contains("sequence length exceeds maximum"));
        }
    }
}
