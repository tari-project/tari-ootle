//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Deserializes a sequence of at most `MAX` elements.
//!
//! The caller names the bound in the field attribute:
//!
//! ```ignore
//! #[serde(deserialize_with = "ootle_serde::bounded_seq::deserialize::<MAX_ITEMS, _, _>")]
//! items: Vec<Item>,
//! ```

use alloc::{boxed::Box, vec::Vec};
use core::{fmt, marker::PhantomData};

use serde::{
    Deserialize,
    Deserializer,
    de::{Error, IgnoredAny, SeqAccess, Visitor},
};

/// The sequence's size hint comes from the input, so it reserves at most this many elements up front.
const MAX_PREALLOC_ELEMENTS: usize = 64;

pub fn deserialize<'de, const MAX: usize, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    deserializer.deserialize_seq(BoundedSeqVisitor::<MAX, T>(PhantomData))
}

pub fn deserialize_boxed<'de, const MAX: usize, T, D>(deserializer: D) -> Result<Box<[T]>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    deserialize::<MAX, T, D>(deserializer).map(Vec::into_boxed_slice)
}

struct BoundedSeqVisitor<const MAX: usize, T>(PhantomData<T>);

impl<'de, const MAX: usize, T: Deserialize<'de>> Visitor<'de> for BoundedSeqVisitor<MAX, T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a sequence of at most {MAX} elements")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where A: SeqAccess<'de> {
        let capacity = seq.size_hint().unwrap_or(0).min(MAX).min(MAX_PREALLOC_ELEMENTS);
        let mut items = Vec::with_capacity(capacity);
        while items.len() < MAX {
            match seq.next_element()? {
                Some(item) => items.push(item),
                None => return Ok(items),
            }
        }
        // A full sequence is still valid if the input ends here. `IgnoredAny` finds out without building the element.
        if seq.next_element::<IgnoredAny>()?.is_some() {
            return Err(Error::invalid_length(MAX.saturating_add(1), &self));
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, vec};

    use serde::de::value::{Error as ValueError, SeqDeserializer};

    use super::*;

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Wrapper {
        #[serde(deserialize_with = "deserialize::<3, _, _>")]
        items: Vec<u32>,
        #[serde(deserialize_with = "deserialize_boxed::<2, _, _>")]
        boxed: Box<[String]>,
    }

    #[test]
    fn it_accepts_up_to_the_bound() {
        let wrapper: Wrapper = serde_json::from_str(r#"{"items":[1,2,3],"boxed":["a","b"]}"#).unwrap();
        assert_eq!(wrapper.items, vec![1, 2, 3]);
        assert_eq!(wrapper.boxed.len(), 2);

        let wrapper: Wrapper = serde_json::from_str(r#"{"items":[],"boxed":[]}"#).unwrap();
        assert!(wrapper.items.is_empty());
        assert!(wrapper.boxed.is_empty());
    }

    #[test]
    fn it_rejects_one_more_than_the_bound() {
        let err = serde_json::from_str::<Wrapper>(r#"{"items":[1,2,3,4],"boxed":[]}"#).unwrap_err();
        assert!(err.to_string().contains("invalid length 4"), "{err}");
        serde_json::from_str::<Wrapper>(r#"{"items":[],"boxed":["a","b","c"]}"#).unwrap_err();
    }

    #[test]
    fn it_rejects_an_excess_element_of_any_shape() {
        serde_json::from_str::<Wrapper>(r#"{"items":[1,2,3,{"not":"a number"}],"boxed":[]}"#).unwrap_err();
    }

    #[test]
    fn it_bounds_a_binary_format() {
        #[derive(serde::Serialize)]
        struct Unbounded {
            items: Vec<u32>,
            boxed: Vec<String>,
        }

        let encode = |items: Vec<u32>| tari_bor::serde_codec::to_vec(Unbounded { items, boxed: vec![] }).unwrap();

        let wrapper: Wrapper = tari_bor::serde_codec::from_slice(&encode(vec![1, 2, 3])).unwrap();
        assert_eq!(wrapper.items, vec![1, 2, 3]);
        tari_bor::serde_codec::from_slice::<Wrapper>(&encode(vec![1, 2, 3, 4])).unwrap_err();
    }

    #[test]
    fn it_caps_the_reservation_made_from_the_size_hint() {
        struct DishonestSeq(u32);

        impl<'de> SeqAccess<'de> for DishonestSeq {
            type Error = ValueError;

            fn next_element_seed<S: serde::de::DeserializeSeed<'de>>(
                &mut self,
                seed: S,
            ) -> Result<Option<S::Value>, Self::Error> {
                if self.0 == 0 {
                    return Ok(None);
                }
                self.0 -= 1;
                seed.deserialize(SeqDeserializer::<_, ValueError>::new(core::iter::empty::<u32>()))
                    .map(Some)
            }

            fn size_hint(&self) -> Option<usize> {
                Some(usize::MAX)
            }
        }

        let items = BoundedSeqVisitor::<{ usize::MAX }, Vec<u32>>(PhantomData)
            .visit_seq(DishonestSeq(2))
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.capacity() <= MAX_PREALLOC_ELEMENTS);
    }
}
