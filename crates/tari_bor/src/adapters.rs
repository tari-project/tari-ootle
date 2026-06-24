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
        mut decode_elem: impl FnMut(&mut Decoder<'b>, &mut C) -> Result<T, minicbor::decode::Error>,
    ) -> Result<Box<[T]>, minicbor::decode::Error> {
        let len = d.array()?;
        match len {
            Some(n) => {
                let mut out = Vec::with_capacity(n.min(super::MAX_PREALLOC) as usize);
                for _ in 0..n {
                    out.push(decode_elem(d, ctx)?);
                }
                Ok(out.into_boxed_slice())
            },
            None => {
                let mut out: Vec<T> = Vec::new();
                loop {
                    if matches!(d.datatype()?, minicbor::data::Type::Break) {
                        d.skip()?;
                        break;
                    }
                    out.push(decode_elem(d, ctx)?);
                }
                Ok(out.into_boxed_slice())
            },
        }
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

/// Adapter that lets `IndexSet<T, S>` participate in minicbor derives via
/// `#[cbor(with = "tari_bor::adapters::indexset_codec")]`.
///
/// On the wire this matches the canonical encoding of `Vec<T>` — a length-prefixed array.
/// On decode the order encoded by the sender is preserved.
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
                    out.insert(v);
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
                    out.insert(v);
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

/// `#[cbor(with = "u128_codec")]` adapter for fields of type `u128`.
///
/// minicbor 2.2 has no built-in `Encode`/`Decode` for `u128` (upstream PR
/// <https://github.com/twittner/minicbor/pull/63> is in flight). Until that lands, this adapter
/// encodes the value using the RFC 8949 §3.4.3 positive-bignum tag — tag `2` wrapping a CBOR
/// byte string of the big-endian magnitude with leading zero bytes stripped (canonical form).
///
/// Switching to the upstream `Encode for u128` once it ships should be wire-compatible: it will
/// produce the same canonical form for values that don't fit in `u64`. (Values that *do* fit in
/// `u64` will likely be emitted as a plain CBOR integer rather than a tagged bignum, so this is
/// not a forever-stable wire format — call it out in the migration when the time comes.)
pub mod u128_codec {
    use minicbor::{CborLen, Decoder, Encoder, data::Tag};

    const TAG_POSITIVE_BIGNUM: u64 = 2;

    fn canonical_bytes(v: u128) -> ([u8; 16], usize) {
        let bytes = v.to_be_bytes();
        // unwrap_or(15) handles v == 0: encode as a single zero byte rather than an empty bstr,
        // since RFC 8949 bignums must be non-empty.
        let first = bytes.iter().position(|&b| b != 0).unwrap_or(15);
        (bytes, first)
    }

    pub fn encode<C, W>(v: &u128, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), minicbor::encode::Error<W::Error>>
    where W: minicbor::encode::Write {
        let (bytes, first) = canonical_bytes(*v);
        e.tag(Tag::new(TAG_POSITIVE_BIGNUM))?;
        e.bytes(&bytes[first..])?;
        Ok(())
    }

    pub fn decode<'b, C>(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<u128, minicbor::decode::Error> {
        let tag: u64 = d.tag()?.into();
        if tag != TAG_POSITIVE_BIGNUM {
            return Err(minicbor::decode::Error::message(
                "u128_codec: expected positive-bignum tag (2)",
            ));
        }
        let bytes = d.bytes()?;
        if bytes.len() > 16 {
            return Err(minicbor::decode::Error::message("u128_codec: bignum exceeds 128 bits"));
        }
        let mut buf = [0u8; 16];
        buf[16 - bytes.len()..].copy_from_slice(bytes);
        Ok(u128::from_be_bytes(buf))
    }

    pub fn cbor_len<C>(v: &u128, ctx: &mut C) -> usize {
        let (_, first) = canonical_bytes(*v);
        let n = 16 - first;
        // Tag header + bstr header (1 byte: payload length 1..=16 fits below the 24-element threshold) + payload.
        <Tag as CborLen<C>>::cbor_len(&Tag::new(TAG_POSITIVE_BIGNUM), ctx) + 1 + n
    }

    #[cfg(test)]
    mod tests {
        use minicbor::{Decoder, Encoder};

        use super::*;

        fn roundtrip(v: u128) {
            let mut buf = Vec::new();
            let mut e = Encoder::new(&mut buf);
            encode(&v, &mut e, &mut ()).unwrap();
            assert_eq!(buf.len(), cbor_len(&v, &mut ()), "cbor_len mismatch for {v}");
            let mut d = Decoder::new(&buf);
            let got = decode::<()>(&mut d, &mut ()).unwrap();
            assert_eq!(got, v);
        }

        #[test]
        fn boundary_values() {
            roundtrip(0);
            roundtrip(1);
            roundtrip(u128::from(u64::MAX));
            roundtrip(u128::from(u64::MAX) + 1);
            roundtrip(u128::MAX);
        }
    }
}

/// Bridges any `serde::Serialize`/`serde::Deserialize` type into minicbor's `#[cbor(with = ...)]`
/// system via our local [`crate::serde_codec`] module (a fork of `minicbor-serde` with `u128`
/// and `i128` support added).
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
