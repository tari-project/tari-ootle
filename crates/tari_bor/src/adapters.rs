//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
//! Shared minicbor `#[cbor(with = ...)]` adapters for container types that don't
//! ship with a derive-friendly codec.

/// Adapter that lets `Box<[T]>` participate in minicbor derives via `#[cbor(with = "boxed_slice")]`.
/// On the wire this matches the canonical encoding of `Vec<T>` — a length-prefixed array.
pub mod boxed_slice {
    #[cfg(not(feature = "std"))]
    use alloc::{boxed::Box, vec::Vec};

    use minicbor::{CborLen, Decode, Decoder, Encode, Encoder};

    pub fn encode<C, T, W>(
        xs: &Box<[T]>,
        e: &mut Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>>
    where
        T: Encode<C>,
        W: minicbor::encode::Write,
    {
        e.array(xs.len() as u64)?;
        for x in xs.as_ref() {
            x.encode(e, ctx)?;
        }
        Ok(())
    }

    pub fn decode<'b, C, T>(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Box<[T]>, minicbor::decode::Error>
    where T: Decode<'b, C> {
        let len = d.array()?;
        match len {
            Some(n) => {
                let mut out = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    out.push(T::decode(d, ctx)?);
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
                    out.push(T::decode(d, ctx)?);
                }
                Ok(out.into_boxed_slice())
            },
        }
    }

    pub fn cbor_len<C, T>(xs: &Box<[T]>, ctx: &mut C) -> usize
    where T: CborLen<C> {
        let n = xs.len() as u64;
        let mut total = <u64 as CborLen<C>>::cbor_len(&n, ctx);
        for x in xs.as_ref() {
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
                let mut out = IndexSet::with_capacity_and_hasher(n as usize, S::default());
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
                let mut out = IndexMap::with_capacity_and_hasher(n as usize, S::default());
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
/// system via the [`minicbor-serde`](https://docs.rs/minicbor-serde) crate.
///
/// Use this on fields whose type is foreign (orphan-rule blocked) and only implements serde —
/// most commonly the consensus proofs from `tari_sidechain`. The subtree is encoded as
/// minicbor-serde would encode it (string-keyed maps for structs), so it does not get the
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
        // minicbor-serde owns its own encoder, so we serialize to a buffer and copy the bytes
        // verbatim into the parent encoder. Cheap enough — typical foreign proofs are < 2KB.
        let bytes = minicbor_serde::to_vec(v)
            .map_err(|err| minicbor::encode::Error::message(format!("serde_bridge encode failed: {err}")))?;
        e.writer_mut().write_all(&bytes).map_err(minicbor::encode::Error::write)
    }

    pub fn decode<'b, C, T>(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<T, minicbor::decode::Error>
    where T: serde::Deserialize<'b> {
        // Skip past the value first so the parent decoder advances correctly, then deserialize
        // from the slice we just walked over. minicbor-serde reads `&'b [u8]`, so the borrow
        // is preserved for zero-copy deserialization where possible.
        let start = d.position();
        d.skip()?;
        let end = d.position();
        let slice = &d.input()[start..end];
        minicbor_serde::from_slice(slice)
            .map_err(|err| minicbor::decode::Error::message(format!("serde_bridge decode failed: {err}")))
    }

    pub fn cbor_len<C, T>(v: &T, _ctx: &mut C) -> usize
    where T: serde::Serialize + ?Sized {
        // The only honest answer is "serialize and measure" since minicbor-serde's wire format
        // depends on the inner type's serde implementation. Pays an extra encode per call.
        minicbor_serde::to_vec(v).map(|bytes| bytes.len()).unwrap_or(0)
    }
}
