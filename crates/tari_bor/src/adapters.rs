//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
//! Shared minicbor `#[cbor(with = ...)]` adapters for container types that don't
//! ship with a derive-friendly codec.

/// Upper bound on how many elements we pre-allocate from an untrusted CBOR length header.
///
/// The length prefix is attacker-controlled, so `with_capacity(n)` lets a tiny payload
/// request a multi-GB allocation (OOM/abort). We reserve at most this many slots up front
/// and let the collection grow as elements are actually decoded — decoding runs out of input
/// long before a dishonest length can allocate unbounded memory. Mirrors the cap the dynamic
/// `Value` decoder already applies in `value.rs`.
const MAX_PREALLOC: u64 = 64;

/// Adapter that lets `Box<[T]>` participate in minicbor derives via `#[cbor(with = "boxed_slice")]`.
/// On the wire this matches the canonical encoding of `Vec<T>` — a length-prefixed array.
pub mod boxed_slice {
    #[cfg(not(feature = "std"))]
    use alloc::{boxed::Box, vec::Vec};

    use minicbor::{CborLen, Decode, Decoder, Encode, Encoder};

    pub fn encode<C, T, W>(xs: &[T], e: &mut Encoder<W>, ctx: &mut C) -> Result<(), minicbor::encode::Error<W::Error>>
    where
        T: Encode<C>,
        W: minicbor::encode::Write,
    {
        e.array(xs.len() as u64)?;
        for x in xs {
            x.encode(e, ctx)?;
        }
        Ok(())
    }

    pub fn decode<'b, C, T>(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Box<[T]>, minicbor::decode::Error>
    where T: Decode<'b, C> {
        decode_with_fn(d, ctx, T::decode)
    }

    /// Like [`decode`], but decodes each element with a caller-supplied function rather than the
    /// element's [`Decode`] impl. This lets a self-recursive type thread decode state — such as a
    /// nesting-depth bound — through the element decode, which a derived `Decode` cannot. The
    /// `MAX_PREALLOC` cap on the untrusted length header still applies.
    pub fn decode_with_fn<'b, C, T>(
        d: &mut Decoder<'b>,
        ctx: &mut C,
        decode_elem: impl FnMut(&mut Decoder<'b>, &mut C) -> Result<T, minicbor::decode::Error>,
    ) -> Result<Box<[T]>, minicbor::decode::Error> {
        super::bounded_vec::decode_with_fn(d, ctx, usize::MAX, decode_elem).map(Vec::into_boxed_slice)
    }

    pub fn cbor_len<C, T>(xs: &[T], ctx: &mut C) -> usize
    where T: CborLen<C> {
        let n = xs.len() as u64;
        let mut total = <u64 as CborLen<C>>::cbor_len(&n, ctx);
        for x in xs {
            total += x.cbor_len(ctx);
        }
        total
    }
}

/// Decodes a `Vec<T>` that may hold at most a caller-chosen number of elements.
///
/// [`MAX_PREALLOC`] stops a dishonest length header from reserving memory, but elements that are really
/// present still decode one by one, and a cheaply encoded element can occupy far more memory decoded than
/// it does on the wire. A field whose element type has that shape wraps [`bounded_vec::decode`] in a
/// `#[cbor(decode_with = ...)]` function that supplies the bound.
pub mod bounded_vec {
    #[cfg(not(feature = "std"))]
    use alloc::{format, vec::Vec};

    use minicbor::{Decode, Decoder};

    /// Decodes an array of at most `max_len` elements. A definite-length array over the bound is refused
    /// from its header, before any element is decoded; an indefinite-length one as soon as it passes it.
    pub fn decode<'b, C, T>(
        d: &mut Decoder<'b>,
        ctx: &mut C,
        max_len: usize,
    ) -> Result<Vec<T>, minicbor::decode::Error>
    where
        T: Decode<'b, C>,
    {
        decode_with_fn(d, ctx, max_len, T::decode)
    }

    /// Like [`decode`], but decodes each element with a caller-supplied function rather than the element's
    /// [`Decode`] impl.
    pub fn decode_with_fn<'b, C, T>(
        d: &mut Decoder<'b>,
        ctx: &mut C,
        max_len: usize,
        mut decode_elem: impl FnMut(&mut Decoder<'b>, &mut C) -> Result<T, minicbor::decode::Error>,
    ) -> Result<Vec<T>, minicbor::decode::Error> {
        let too_many = || minicbor::decode::Error::message(format!("array holds more than {max_len} elements"));
        match d.array()? {
            Some(n) => {
                if n > max_len as u64 {
                    return Err(too_many());
                }
                let mut out = Vec::with_capacity(n.min(super::MAX_PREALLOC) as usize);
                for _ in 0..n {
                    out.push(decode_elem(d, ctx)?);
                }
                Ok(out)
            },
            None => {
                let mut out = Vec::new();
                loop {
                    if matches!(d.datatype()?, minicbor::data::Type::Break) {
                        d.skip()?;
                        return Ok(out);
                    }
                    if out.len() == max_len {
                        return Err(too_many());
                    }
                    out.push(decode_elem(d, ctx)?);
                }
            },
        }
    }
}

/// Adapter that lets `IndexSet<T, S>` participate in minicbor derives via
/// `#[cbor(with = "tari_bor::adapters::indexset_codec")]`.
///
/// On the wire this matches the canonical encoding of `Vec<T>` — a length-prefixed array.
/// On decode the order encoded by the sender is preserved, and an element encoded twice is an error: a set
/// has no single meaning for it.
#[cfg(feature = "indexmap")]
pub mod indexset_codec {
    use core::hash::{BuildHasher, Hash};

    use indexmap::IndexSet;
    use minicbor::{CborLen, Decode, Decoder, Encode, Encoder};

    pub fn encode<C, T, S, W>(
        m: &IndexSet<T, S>,
        e: &mut Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>>
    where
        T: Encode<C>,
        W: minicbor::encode::Write,
    {
        e.array(m.len() as u64)?;
        for v in m {
            v.encode(e, ctx)?;
        }
        Ok(())
    }

    pub fn decode<'b, C, T, S>(d: &mut Decoder<'b>, ctx: &mut C) -> Result<IndexSet<T, S>, minicbor::decode::Error>
    where
        T: Decode<'b, C> + Hash + Eq,
        S: BuildHasher + Default,
    {
        let len = d.array()?;
        match len {
            Some(n) => {
                let mut out = IndexSet::with_capacity_and_hasher(n.min(super::MAX_PREALLOC) as usize, S::default());
                for _ in 0..n {
                    let v = T::decode(d, ctx)?;
                    if !out.insert(v) {
                        return Err(minicbor::decode::Error::message("duplicate element in set"));
                    }
                }
                Ok(out)
            },
            None => {
                let mut out = IndexSet::with_hasher(S::default());
                loop {
                    if matches!(d.datatype()?, minicbor::data::Type::Break) {
                        d.skip()?;
                        break;
                    }
                    let v = T::decode(d, ctx)?;
                    if !out.insert(v) {
                        return Err(minicbor::decode::Error::message("duplicate element in set"));
                    }
                }
                Ok(out)
            },
        }
    }

    pub fn cbor_len<C, T, S>(m: &IndexSet<T, S>, ctx: &mut C) -> usize
    where T: CborLen<C> {
        let n = m.len() as u64;
        let mut total = <u64 as CborLen<C>>::cbor_len(&n, ctx);
        for v in m {
            total += v.cbor_len(ctx);
        }
        total
    }
}

/// Adapter that lets `IndexMap<K, V, S>` participate in minicbor derives via
/// `#[cbor(with = "tari_bor::adapters::indexmap_codec")]`.
///
/// On the wire this uses the standard CBOR map type, mirroring the encoding produced
/// by `BTreeMap<K, V>`. On decode, the entries are inserted in iteration order so the
/// resulting `IndexMap` preserves the order encoded by the sender.
#[cfg(feature = "indexmap")]
pub mod indexmap_codec {
    use core::hash::{BuildHasher, Hash};

    use indexmap::IndexMap;
    use minicbor::{CborLen, Decode, Decoder, Encode, Encoder};

    pub fn encode<C, K, V, S, W>(
        m: &IndexMap<K, V, S>,
        e: &mut Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>>
    where
        K: Encode<C>,
        V: Encode<C>,
        W: minicbor::encode::Write,
    {
        e.map(m.len() as u64)?;
        for (k, v) in m {
            k.encode(e, ctx)?;
            v.encode(e, ctx)?;
        }
        Ok(())
    }

    pub fn decode<'b, C, K, V, S>(
        d: &mut Decoder<'b>,
        ctx: &mut C,
    ) -> Result<IndexMap<K, V, S>, minicbor::decode::Error>
    where
        K: Decode<'b, C> + Hash + Eq,
        V: Decode<'b, C>,
        S: BuildHasher + Default,
    {
        let len = d.map()?;
        match len {
            Some(n) => {
                let mut out = IndexMap::with_capacity_and_hasher(n.min(super::MAX_PREALLOC) as usize, S::default());
                for _ in 0..n {
                    let k = K::decode(d, ctx)?;
                    let v = V::decode(d, ctx)?;
                    out.insert(k, v);
                }
                Ok(out)
            },
            None => {
                let mut out = IndexMap::with_hasher(S::default());
                loop {
                    if matches!(d.datatype()?, minicbor::data::Type::Break) {
                        d.skip()?;
                        break;
                    }
                    let k = K::decode(d, ctx)?;
                    let v = V::decode(d, ctx)?;
                    out.insert(k, v);
                }
                Ok(out)
            },
        }
    }

    pub fn cbor_len<C, K, V, S>(m: &IndexMap<K, V, S>, ctx: &mut C) -> usize
    where
        K: CborLen<C>,
        V: CborLen<C>,
    {
        let n = m.len() as u64;
        let mut total = <u64 as CborLen<C>>::cbor_len(&n, ctx);
        for (k, v) in m {
            total += k.cbor_len(ctx);
            total += v.cbor_len(ctx);
        }
        total
    }
}

/// Bridges any `serde::Serialize`/`serde::Deserialize` type into minicbor's `#[cbor(with = ...)]`
/// system via our local [`crate::serde_codec`] module (a fork of `minicbor-serde` that also
/// accepts a CBOR byte string where serde asks for a sequence).
///
/// Use this on fields whose type is foreign (orphan-rule blocked) and only implements serde —
/// most commonly the consensus proofs from `tari_sidechain`. The subtree is encoded as
/// `serde_codec` would encode it (string-keyed maps for structs), so it does not get the
/// integer-tag size win, but it round-trips without requiring upstream changes.
#[cfg(feature = "serde")]
pub mod serde_bridge {
    #[cfg(not(feature = "std"))]
    use alloc::format;

    use minicbor::{Decoder, Encoder};

    pub fn encode<C, T, W>(v: &T, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), minicbor::encode::Error<W::Error>>
    where
        T: serde::Serialize + ?Sized,
        W: minicbor::encode::Write,
    {
        // serde_codec owns its own encoder, so we serialize to a buffer and copy the bytes
        // verbatim into the parent encoder. Cheap enough — typical foreign proofs are < 2KB.
        let bytes = crate::serde_codec::to_vec(v)
            .map_err(|err| minicbor::encode::Error::message(format!("serde_bridge encode failed: {err}")))?;
        e.writer_mut().write_all(&bytes).map_err(minicbor::encode::Error::write)
    }

    pub fn decode<'b, C, T>(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<T, minicbor::decode::Error>
    where T: serde::Deserialize<'b> {
        // Skip past the value first so the parent decoder advances correctly, then deserialize
        // from the slice we just walked over. serde_codec reads `&'b [u8]`, so the borrow is
        // preserved for zero-copy deserialization where possible.
        let start = d.position();
        d.skip()?;
        let end = d.position();
        let slice = &d.input()[start..end];
        crate::serde_codec::from_slice(slice)
            .map_err(|err| minicbor::decode::Error::message(format!("serde_bridge decode failed: {err}")))
    }

    pub fn cbor_len<C, T>(v: &T, _ctx: &mut C) -> usize
    where T: serde::Serialize + ?Sized {
        // Wire format depends on the inner type's serde impl, so we still have to drive a full
        // serialize. ByteCounter implements minicbor::encode::Write but discards the bytes, so
        // we avoid the Vec allocation an honest `to_vec(v).len()` would pay.
        //
        // We panic on serialize failure: if the foreign type can't serialize via serde, the
        // downstream `encode` call would also fail and produce wrong wire bytes, so silently
        // returning 0 (and letting the caller allocate too-small buffers) would hide the bug.
        let mut counter = crate::ByteCounter::new();
        let mut ser = crate::serde_codec::Serializer::new(&mut counter);
        v.serialize(&mut ser)
            .expect("serde_bridge cbor_len: foreign-type serialize failed");
        counter.get()
    }
}

#[cfg(test)]
mod alloc_cap_tests {
    use minicbor::Decoder;

    // CBOR array/map headers claiming u64::MAX elements with no element bytes following. With the
    // pre-allocation capped, decode fails cleanly on the missing input; an unbounded
    // `with_capacity(n)` would instead reserve a multi-GB buffer from this ~9-byte payload and abort.
    const HUGE_ARRAY_HEADER: [u8; 9] = [0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    #[cfg(feature = "indexmap")]
    const HUGE_MAP_HEADER: [u8; 9] = [0xbb, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];

    #[test]
    fn boxed_slice_rejects_dishonest_length_without_oom() {
        let mut d = Decoder::new(&HUGE_ARRAY_HEADER);
        assert!(super::boxed_slice::decode::<(), u8>(&mut d, &mut ()).is_err());
    }

    #[cfg(feature = "indexmap")]
    #[test]
    fn indexset_rejects_dishonest_length_without_oom() {
        use std::collections::hash_map::RandomState;
        let mut d = Decoder::new(&HUGE_ARRAY_HEADER);
        assert!(super::indexset_codec::decode::<(), u8, RandomState>(&mut d, &mut ()).is_err());
    }

    #[cfg(feature = "indexmap")]
    #[test]
    fn indexmap_rejects_dishonest_length_without_oom() {
        use std::collections::hash_map::RandomState;
        let mut d = Decoder::new(&HUGE_MAP_HEADER);
        assert!(super::indexmap_codec::decode::<(), u8, u8, RandomState>(&mut d, &mut ()).is_err());
    }
}

#[cfg(test)]
mod bounded_vec_tests {
    use minicbor::Decoder;

    use super::bounded_vec;

    #[test]
    fn an_array_at_the_bound_decodes() {
        // [1, 2] as a definite-length and an indefinite-length array.
        for bytes in [&[0x82, 0x01, 0x02][..], &[0x9f, 0x01, 0x02, 0xff][..]] {
            let mut d = Decoder::new(bytes);
            assert_eq!(bounded_vec::decode::<(), u8>(&mut d, &mut (), 2).unwrap(), vec![1, 2]);
        }
    }

    #[test]
    fn an_array_over_the_bound_is_refused() {
        // [1, 2, 3] as a definite-length and an indefinite-length array.
        for bytes in [&[0x83, 0x01, 0x02, 0x03][..], &[0x9f, 0x01, 0x02, 0x03, 0xff][..]] {
            let mut d = Decoder::new(bytes);
            assert!(bounded_vec::decode::<(), u8>(&mut d, &mut (), 2).is_err());
        }
    }

    #[test]
    fn a_definite_length_over_the_bound_is_refused_from_its_header() {
        // Claims three elements and carries none: refusing it must not need them.
        let mut d = Decoder::new(&[0x83]);
        let err = bounded_vec::decode::<(), u8>(&mut d, &mut (), 2).unwrap_err();
        assert!(!err.is_end_of_input(), "{err}");
    }
}

#[cfg(all(test, feature = "indexmap"))]
mod indexset_codec_tests {
    use std::collections::hash_map::RandomState;

    use minicbor::Decoder;

    use super::indexset_codec;

    #[test]
    fn a_set_with_a_repeated_element_is_rejected() {
        // [1, 2, 1] as a definite-length and an indefinite-length array.
        for bytes in [&[0x83, 0x01, 0x02, 0x01][..], &[0x9f, 0x01, 0x02, 0x01, 0xff][..]] {
            let mut d = Decoder::new(bytes);
            assert!(indexset_codec::decode::<(), u8, RandomState>(&mut d, &mut ()).is_err());
        }

        let mut d = Decoder::new(&[0x82, 0x01, 0x02]);
        let set = indexset_codec::decode::<(), u8, RandomState>(&mut d, &mut ()).unwrap();
        assert_eq!(set.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }
}
