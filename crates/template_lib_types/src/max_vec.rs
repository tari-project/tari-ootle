//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{
    fmt,
    format,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    prelude::*,
    vec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize), serde(transparent))]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct MaxVec<const N: usize, T> {
    elems: Box<[T]>,
}

#[cfg(feature = "borsh")]
impl<const N: usize, T: borsh::BorshDeserialize> borsh::BorshDeserialize for MaxVec<N, T> {
    fn deserialize_reader<R: borsh::io::Read>(reader: &mut R) -> Result<Self, borsh::io::Error> {
        let len = u32::deserialize_reader(reader)? as usize;
        if len > N {
            return Err(borsh::io::Error::new(
                borsh::io::ErrorKind::InvalidData,
                format!("sequence length exceeds maximum of {}: got {}", N, len),
            ));
        }
        let mut elems = Vec::with_capacity(len.min(64));
        for _ in 0..len {
            elems.push(T::deserialize_reader(reader)?);
        }
        Ok(Self {
            elems: elems.into_boxed_slice(),
        })
    }
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
        let elems = elems.into();
        debug_assert!(
            elems.len() <= N,
            "sequence length exceeds maximum of {}: got {}",
            N,
            elems.len()
        );
        Self { elems }
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

#[cfg(feature = "serde")]
struct MaxVecVisitor<const N: usize, T>(PhantomData<T>);

#[cfg(feature = "serde")]
impl<'de, const N: usize, T: serde::Deserialize<'de>> serde::de::Visitor<'de> for MaxVecVisitor<N, T> {
    type Value = MaxVec<N, T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a sequence of at most {} elements", N)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where A: serde::de::SeqAccess<'de> {
        if let Some(hint) = seq.size_hint() &&
            hint > N
        {
            return Err(serde::de::Error::custom(format!(
                "sequence length exceeds maximum of {}: got {}",
                N, hint
            )));
        }
        let mut elems = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(N).min(64));
        while let Some(elem) = seq.next_element()? {
            if elems.len() == N {
                return Err(serde::de::Error::custom(format!(
                    "sequence length exceeds maximum of {}",
                    N
                )));
            }
            elems.push(elem);
        }
        Ok(MaxVec {
            elems: elems.into_boxed_slice(),
        })
    }
}

#[cfg(feature = "serde")]
impl<'de, const N: usize, T: serde::Deserialize<'de>> serde::Deserialize<'de> for MaxVec<N, T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        deserializer.deserialize_seq(MaxVecVisitor(PhantomData))
    }
}

impl<C, const N: usize, T> minicbor::Encode<C> for MaxVec<N, T>
where T: minicbor::Encode<C>
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(self.elems.len() as u64)?;
        for elem in self.elems.as_ref() {
            elem.encode(e, ctx)?;
        }
        Ok(())
    }
}

impl<'b, C, const N: usize, T> minicbor::Decode<'b, C> for MaxVec<N, T>
where T: minicbor::Decode<'b, C>
{
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?;
        match len {
            Some(n) => {
                if n > N as u64 {
                    return Err(minicbor::decode::Error::message(format!(
                        "sequence length exceeds maximum of {}: got {}",
                        N, n
                    )));
                }
                let count = n as usize;
                let mut elems = Vec::with_capacity(count.min(64));
                for _ in 0..count {
                    elems.push(T::decode(d, ctx)?);
                }
                Ok(MaxVec {
                    elems: elems.into_boxed_slice(),
                })
            },
            None => {
                let mut elems: Vec<T> = Vec::new();
                loop {
                    if matches!(d.datatype()?, minicbor::data::Type::Break) {
                        d.skip()?;
                        break;
                    }
                    if elems.len() == N {
                        return Err(minicbor::decode::Error::message(format!(
                            "sequence length exceeds maximum of {}",
                            N
                        )));
                    }
                    elems.push(T::decode(d, ctx)?);
                }
                Ok(MaxVec {
                    elems: elems.into_boxed_slice(),
                })
            },
        }
    }
}

impl<C, const N: usize, T> minicbor::CborLen<C> for MaxVec<N, T>
where T: minicbor::CborLen<C>
{
    fn cbor_len(&self, ctx: &mut C) -> usize {
        let n = self.elems.len() as u64;
        let mut total = <u64 as minicbor::CborLen<C>>::cbor_len(&n, ctx);
        for elem in self.elems.as_ref() {
            total += elem.cbor_len(ctx);
        }
        total
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

    #[cfg(feature = "serde")]
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

        #[test]
        fn it_stops_deserializing_once_limit_is_exceeded() {
            struct PanicElement;
            impl<'de> serde::Deserialize<'de> for PanicElement {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where D: serde::Deserializer<'de> {
                    let v = u8::deserialize(deserializer)?;
                    assert!(v <= 3, "deserializer must abort at the 3rd element before reaching element {v}");
                    Ok(Self)
                }
            }

            let json = "[1,2,3,4,5,6]";
            let err = serde_json::from_str::<MaxVec<2, PanicElement>>(json)
                .err()
                .expect("expected length error");
            assert!(err.to_string().contains("sequence length exceeds maximum"));
        }
    }

    #[cfg(feature = "borsh")]
    mod borsh_impl {
        use borsh::{BorshDeserialize, BorshSerialize};

        use super::*;

        #[test]
        fn it_enforces_max_length_on_borsh_deserialize() {
            let valid = MaxVec::<3, u16>::new_checked(vec![10, 20, 30]).unwrap();
            let mut encoded = Vec::new();
            valid.serialize(&mut encoded).unwrap();
            let decoded = MaxVec::<3, u16>::try_from_slice(&encoded).unwrap();
            assert_eq!(decoded, valid);

            // Header claiming u32::MAX elements on a 4-byte buffer fails on the bound check before reading elements.
            let oversized_header = u32::MAX.to_le_bytes();
            let err = MaxVec::<3, u16>::try_from_slice(&oversized_header).unwrap_err();
            assert_eq!(err.kind(), borsh::io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("sequence length exceeds maximum"));
        }
    }
}
