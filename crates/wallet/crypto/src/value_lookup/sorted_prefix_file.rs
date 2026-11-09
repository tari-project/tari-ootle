//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Sorted prefix-index value lookup (`VLK2`).
//!
//! Where [`MMapValueLookup`](crate::MMapValueLookup) is a dense array indexed by value (reverse lookup is an
//! O(n) scan, value recovered from the file offset), this format stores `(point_prefix, value)` records
//! **sorted by point prefix**, so reverse lookup is an O(log n) binary search.
//!
//! Sorting by point destroys the positional encoding of the value, so the value is stored explicitly — but
//! only a short *prefix* of the 32-byte point is stored as the search key. Exactness is restored by
//! recomputing `v·G` for the matched value and comparing the full point against the target, which rejects
//! the (≈ `count / 2^{8·prefix_len}`) prefix collisions. Out-of-range targets fail the prefix comparison
//! without a recompute.

use std::{cmp::Ordering, convert::Infallible, fs::File, io, io::Write, mem::size_of, ops::RangeInclusive};

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::crypto::ValueLookup;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

/// Magic bytes identifying a Tari value lookup table file in the self-describing family. The byte that
/// follows names the concrete [`LookupFormat`], so a reader identifies the layout from the header alone.
/// Distinct from the legacy dense `VLKP` format.
pub const LOOKUP_MAGIC: &[u8] = b"VLKT";

/// Default number of leading point bytes stored as the search key. 8 bytes (64 bits) keeps the expected
/// prefix-collision count per lookup (`count / 2^64`) negligible for any realistic table size.
pub const DEFAULT_PREFIX_LEN: u8 = 8;

/// Self-describing lookup file format, recorded in the header immediately after [`LOOKUP_MAGIC`].
///
/// Only [`LookupFormat::SortedPrefixV1`] is produced and read today. The field exists so that a future
/// format can be detected — and this reader can reject it with a clear error — without changing the magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LookupFormat {
    /// Records of `(point_prefix, value_offset)` sorted ascending by prefix; O(log n) reverse lookup.
    SortedPrefixV1 = 1,
}

impl LookupFormat {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::SortedPrefixV1),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Reads the [`LookupFormat`] declared in a file header without validating the rest of the layout.
pub fn read_lookup_format(buf: &[u8]) -> io::Result<LookupFormat> {
    let buf = buf
        .get(..LOOKUP_MAGIC.len() + 1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Buffer too small for lookup header"))?;
    if &buf[..LOOKUP_MAGIC.len()] != LOOKUP_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid value lookup header magic bytes",
        ));
    }
    let format_byte = buf[LOOKUP_MAGIC.len()];
    LookupFormat::from_byte(format_byte).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unsupported value lookup format id {format_byte}"),
        )
    })
}

/// Fixed-size header preceding the sorted `(prefix, value_offset)` records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortedLookupHeader {
    pub min: u64,
    pub max: u64,
    /// Number of leading point bytes stored per record as the search key (`1..=32`).
    pub prefix_len: u8,
    /// Number of bytes used to store each value offset `v - min`, little-endian (`1..=8`).
    pub value_len: u8,
}

impl SortedLookupHeader {
    pub const FORMAT: LookupFormat = LookupFormat::SortedPrefixV1;
    /// `magic (4) + format (1) + min (8) + max (8) + prefix_len (1) + value_len (1)`.
    pub const SIZE: usize = LOOKUP_MAGIC.len() + 1 + size_of::<u64>() * 2 + 2;

    pub fn new(min: u64, max: u64, prefix_len: u8, value_len: u8) -> Self {
        Self {
            min,
            max,
            prefix_len,
            value_len,
        }
    }

    /// Minimum number of bytes needed to store any value offset in `0..=(max - min)`.
    pub fn required_value_len(min: u64, max: u64) -> u8 {
        let span = max.saturating_sub(min);
        let bits = u64::BITS - span.leading_zeros();
        (bits.div_ceil(8)).max(1) as u8
    }

    /// Number of bytes per record.
    pub const fn stride(&self) -> usize {
        self.prefix_len as usize + self.value_len as usize
    }

    /// Number of records (== number of values covered).
    pub fn count(&self) -> u64 {
        self.max.saturating_sub(self.min).saturating_add(1)
    }

    pub fn encode_into<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(LOOKUP_MAGIC)?;
        writer.write_all(&[Self::FORMAT.as_byte()])?;
        writer.write_all(&self.min.to_be_bytes())?;
        writer.write_all(&self.max.to_be_bytes())?;
        writer.write_all(&[self.prefix_len, self.value_len])?;
        Ok(())
    }

    pub fn from_buf(buf: &[u8]) -> io::Result<Self> {
        // Validates the magic and that the declared format is the one this reader supports.
        if read_lookup_format(buf)? != Self::FORMAT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Value lookup file is not in the sorted prefix format",
            ));
        }
        let buf = buf
            .get(..Self::SIZE)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Buffer too small for sorted lookup header"))?;
        let body = &buf[LOOKUP_MAGIC.len() + 1..];
        let min = u64::from_be_bytes(body[..8].try_into().expect("slice length checked"));
        let max = u64::from_be_bytes(body[8..16].try_into().expect("slice length checked"));
        let prefix_len = body[16];
        let value_len = body[17];
        Ok(Self {
            min,
            max,
            prefix_len,
            value_len,
        })
    }

    pub fn is_in_range(&self, value: u64) -> bool {
        value >= self.min && value <= self.max
    }
}

/// Computes the 32-byte compressed encoding of `v·G` (the value the lookup table maps `v` to).
fn compute_point_bytes(value: u64) -> RistrettoPublicKeyBytes {
    let pk = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(value));
    pk.to_byte_type()
}

/// A memory-mapped, sorted prefix-index value lookup supporting O(log n) reverse lookup.
///
/// The mapped data is immutable after load and accessed through `&self`, so a single instance can be shared
/// across threads for concurrent lookups.
pub struct SortedPrefixFileLookup {
    mmap: memmap2::Mmap,
    header: SortedLookupHeader,
    count: usize,
}

impl SortedPrefixFileLookup {
    /// Loads a sorted value lookup table from the specified file.
    ///
    /// # Safety
    /// This function memory-maps the file. The caller must ensure the file is not modified while it is in use.
    pub unsafe fn load(file: &File) -> io::Result<Self> {
        // Safety: forwarded to the caller — the file must not be modified while mapped.
        let mmap = unsafe { memmap2::Mmap::map(file)? };
        Self::from_mmap(mmap)
    }

    pub fn from_buf(buf: &[u8]) -> io::Result<Self> {
        let mut mmap = memmap2::MmapOptions::new().len(buf.len()).map_anon()?;
        mmap[..buf.len()].copy_from_slice(buf);
        Self::from_mmap(mmap.make_read_only()?)
    }

    fn from_mmap(mmap: memmap2::Mmap) -> io::Result<Self> {
        let header = SortedLookupHeader::from_buf(&mmap)?;
        if header.min > header.max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Header min is greater than max",
            ));
        }
        if !(1..=32).contains(&header.prefix_len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "prefix_len must be in 1..=32",
            ));
        }
        if !(1..=8).contains(&header.value_len) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "value_len must be in 1..=8"));
        }
        let required_value_len = SortedLookupHeader::required_value_len(header.min, header.max);
        if header.value_len < required_value_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Header value_len {} is too small for range {}..={} (requires at least {} bytes)",
                    header.value_len, header.min, header.max, required_value_len
                ),
            ));
        }

        let count = header
            .max
            .checked_sub(header.min)
            .and_then(|span| span.checked_add(1))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Table value range overflow"))?;
        let expected_data = count
            .checked_mul(header.stride() as u64)
            .and_then(|d| d.checked_add(SortedLookupHeader::SIZE as u64))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Table size overflow"))?;
        if mmap.len() as u64 != expected_data {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Sorted lookup file size mismatch: expected {} bytes, found {}",
                    expected_data,
                    mmap.len()
                ),
            ));
        }

        Ok(Self {
            mmap,
            header,
            count: count as usize,
        })
    }

    /// Returns the supported value range of the lookup table.
    pub fn range(&self) -> RangeInclusive<u64> {
        self.header.min..=self.header.max
    }

    pub fn header(&self) -> &SortedLookupHeader {
        &self.header
    }

    /// Number of records in the table.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns the `(prefix, value)` of the record at index `i` in stored (prefix-sorted) order.
    ///
    /// # Panics
    /// Panics if `i >= len()`.
    pub fn entry(&self, i: usize) -> (&[u8], u64) {
        assert!(i < self.count, "entry index out of bounds");
        (self.prefix_at(i), self.value_at(i))
    }

    const fn record_start(&self, i: usize) -> usize {
        SortedLookupHeader::SIZE + i * self.header.stride()
    }

    fn prefix_at(&self, i: usize) -> &[u8] {
        let start = self.record_start(i);
        &self.mmap[start..start + self.header.prefix_len as usize]
    }

    fn value_at(&self, i: usize) -> u64 {
        let value_len = self.header.value_len as usize;
        let start = self.record_start(i) + self.header.prefix_len as usize;
        let mut buf = [0u8; 8];
        buf[..value_len].copy_from_slice(&self.mmap[start..start + value_len]);
        // Offsets are relative to `min` and must stay within `..=max`; a corrupt record clamps rather than panics.
        self.header
            .min
            .saturating_add(u64::from_le_bytes(buf))
            .min(self.header.max)
    }

    /// Index of the first record whose prefix is `>= key` (binary search lower bound).
    fn lower_bound(&self, key: &[u8]) -> usize {
        let mut lo = 0usize;
        let mut hi = self.count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.prefix_at(mid).cmp(key) {
                Ordering::Less => lo = mid + 1,
                Ordering::Equal | Ordering::Greater => hi = mid,
            }
        }
        lo
    }

    /// Reverse lookup: returns the value `v` in `[min, max]` such that `v·G == target`, or `None`.
    pub fn find_value(&self, target: &RistrettoPublicKeyBytes) -> Option<u64> {
        let key = &target[..self.header.prefix_len as usize];
        let mut i = self.lower_bound(key);
        // Scan the (usually singleton) run of records sharing this prefix, verifying the full point.
        while i < self.count && self.prefix_at(i) == key {
            let v = self.value_at(i);
            if compute_point_bytes(v) == *target {
                return Some(v);
            }
            i += 1;
        }
        None
    }
}

impl ValueLookup for SortedPrefixFileLookup {
    type Error = Infallible;

    fn lookup(&self, point: &RistrettoPublicKeyBytes) -> Result<Option<u64>, Self::Error> {
        Ok(self.find_value(point))
    }
}

#[cfg(test)]
mod tests {
    use rand::RngExt;

    use super::*;

    /// Generates the `(prefix, value_offset)` records for `min..=max`, sorts them by prefix, and writes a
    /// complete `VLK2` buffer.
    ///
    /// This holds all records in memory, so it is intended for tests and small tables. The generator utility
    /// produces large tables with a bucketed, parallel variant.
    fn write_sorted_lookup_in_memory<W: Write>(writer: &mut W, min: u64, max: u64, prefix_len: u8) -> io::Result<()> {
        assert!(min <= max, "min must be <= max");
        assert!((1..=32).contains(&prefix_len), "prefix_len must be in 1..=32");

        let value_len = SortedLookupHeader::required_value_len(min, max);
        let header = SortedLookupHeader::new(min, max, prefix_len, value_len);
        header.encode_into(writer)?;

        let prefix_len = prefix_len as usize;
        let value_len = value_len as usize;

        let mut entries = (min..=max)
            .map(|v| (compute_point_bytes(v), v - min))
            .collect::<Vec<_>>();
        entries.sort_unstable_by(|a, b| a.0[..prefix_len].cmp(&b.0[..prefix_len]));

        let mut record = vec![0u8; prefix_len + value_len];
        for (point, offset) in entries {
            record[..prefix_len].copy_from_slice(&point[..prefix_len]);
            record[prefix_len..].copy_from_slice(&offset.to_le_bytes()[..value_len]);
            writer.write_all(&record)?;
        }
        Ok(())
    }

    fn build(min: u64, max: u64) -> SortedPrefixFileLookup {
        let mut buf = Vec::new();
        write_sorted_lookup_in_memory(&mut buf, min, max, DEFAULT_PREFIX_LEN).unwrap();
        SortedPrefixFileLookup::from_buf(&buf).unwrap()
    }

    #[test]
    fn it_rejects_a_full_range_header() {
        let mut buf = Vec::new();
        SortedLookupHeader::new(0, u64::MAX, DEFAULT_PREFIX_LEN, 8)
            .encode_into(&mut buf)
            .unwrap();
        let Err(err) = SortedPrefixFileLookup::from_buf(&buf) else {
            panic!("expected the full-range header to be rejected");
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn it_reports_the_range() {
        let lookup = build(10, 100);
        assert_eq!(lookup.range(), 10..=100);
    }

    #[test]
    fn it_finds_every_value_in_range() {
        const MIN: u64 = 1000;
        const MAX: u64 = 5000;
        let lookup = build(MIN, MAX);
        for v in MIN..=MAX {
            let target = compute_point_bytes(v);
            assert_eq!(lookup.find_value(&target), Some(v), "failed at {v}");
        }
    }

    #[test]
    fn it_returns_none_for_values_outside_the_range() {
        let lookup = build(1000, 2000);
        for v in [0u64, 1, 999, 2001, 10_000, u64::MAX] {
            let target = compute_point_bytes(v);
            assert_eq!(lookup.find_value(&target), None, "unexpected hit at {v}");
        }
    }

    #[test]
    fn it_handles_a_single_element_table() {
        let lookup = build(42, 42);
        assert_eq!(lookup.find_value(&compute_point_bytes(42)), Some(42));
        assert_eq!(lookup.find_value(&compute_point_bytes(41)), None);
        assert_eq!(lookup.find_value(&compute_point_bytes(43)), None);
    }

    #[test]
    fn it_finds_random_values() {
        const MIN: u64 = 0;
        const MAX: u64 = 20_000;
        let lookup = build(MIN, MAX);
        let mut rng = rand::rng();
        for _ in 0..2000 {
            let v = rng.random_range(MIN..=MAX);
            assert_eq!(lookup.find_value(&compute_point_bytes(v)), Some(v), "failed at {v}");
        }
    }

    #[test]
    fn it_reads_the_generator_key_encoding() {
        // Mirrors the generator's parallel path: sort key is the first 8 point bytes read big-endian, and the
        // stored prefix is `key.to_be_bytes()[..prefix_len]`. This must round-trip through the reader, which
        // compares raw prefix bytes lexicographically.
        const MIN: u64 = 0;
        const MAX: u64 = 8_000;
        let value_len = SortedLookupHeader::required_value_len(MIN, MAX);
        let prefix_len = DEFAULT_PREFIX_LEN as usize;

        let mut entries: Vec<(u64, u64)> = (MIN..=MAX)
            .map(|v| {
                let point = compute_point_bytes(v);
                let mut key = [0u8; 8];
                key.copy_from_slice(&point[..8]);
                (u64::from_be_bytes(key), v - MIN)
            })
            .collect();
        entries.sort_unstable();

        let mut buf = Vec::new();
        SortedLookupHeader::new(MIN, MAX, DEFAULT_PREFIX_LEN, value_len)
            .encode_into(&mut buf)
            .unwrap();
        for (key, offset) in entries {
            buf.extend_from_slice(&key.to_be_bytes()[..prefix_len]);
            buf.extend_from_slice(&offset.to_le_bytes()[..value_len as usize]);
        }

        let lookup = SortedPrefixFileLookup::from_buf(&buf).unwrap();
        for v in MIN..=MAX {
            assert_eq!(lookup.find_value(&compute_point_bytes(v)), Some(v), "failed at {v}");
        }
        assert_eq!(lookup.find_value(&compute_point_bytes(MAX + 1)), None);
    }

    #[test]
    fn required_value_len_is_correct() {
        assert_eq!(SortedLookupHeader::required_value_len(0, 0), 1);
        assert_eq!(SortedLookupHeader::required_value_len(0, 255), 1);
        assert_eq!(SortedLookupHeader::required_value_len(0, 256), 2);
        assert_eq!(SortedLookupHeader::required_value_len(0, 65_535), 2);
        assert_eq!(SortedLookupHeader::required_value_len(0, 65_536), 3);
        assert_eq!(SortedLookupHeader::required_value_len(0, 10_000_000_000), 5);
        // Offset is relative to min, so a high but narrow range stays small.
        assert_eq!(SortedLookupHeader::required_value_len(1_000_000, 1_000_255), 1);
    }

    #[test]
    fn rejects_a_truncated_file() {
        let mut buf = Vec::new();
        write_sorted_lookup_in_memory(&mut buf, 0, 100, DEFAULT_PREFIX_LEN).unwrap();
        buf.truncate(buf.len() - 1);
        assert!(SortedPrefixFileLookup::from_buf(&buf).is_err());
    }

    #[test]
    fn rejects_value_len_too_small_for_range() {
        // Range 0..=256 requires at least 2 bytes per value offset.
        let min = 0u64;
        let max = 256u64;
        let prefix_len = DEFAULT_PREFIX_LEN;
        let invalid_value_len = 1u8;
        let mut buf = Vec::new();
        let header = SortedLookupHeader::new(min, max, prefix_len, invalid_value_len);
        header.encode_into(&mut buf).unwrap();
        buf.resize(
            SortedLookupHeader::SIZE + ((max - min + 1) as usize) * header.stride(),
            0u8,
        );
        let Err(err) = SortedPrefixFileLookup::from_buf(&buf) else {
            panic!("expected header with value_len too small for range to be rejected");
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
