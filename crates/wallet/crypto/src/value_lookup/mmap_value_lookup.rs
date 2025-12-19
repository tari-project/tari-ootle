//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fs::File, io, ops::RangeInclusive};

use tari_engine_types::crypto::{AndThenLookup, MapErrLookup, ValueLookupTable};

use crate::value_lookup::header::LookupHeader;

pub struct MMapValueLookup {
    mmap: memmap2::Mmap,
    header: LookupHeader,
    pos: usize,
    last_value: u64,
}

impl MMapValueLookup {
    /// Loads a memory-mapped value lookup table from the specified file.
    ///
    /// # Safety
    /// This function uses unsafe code to create a memory-mapped file. The caller must ensure that the file is not
    /// modified while it is being used.
    pub unsafe fn load(file: &File) -> io::Result<Self> {
        let mmap = memmap2::Mmap::map(file)?;
        let header = LookupHeader::from_buf(&mmap)?;
        #[cfg(unix)]
        mmap.advise_range(
            memmap2::Advice::Sequential,
            LookupHeader::SIZE,
            mmap.len() - LookupHeader::SIZE,
        )?;

        Ok(Self {
            mmap,
            header,
            pos: 0,
            last_value: 0,
        })
    }

    pub fn from_buf(buf: &[u8]) -> io::Result<Self> {
        let mut cursor = io::Cursor::new(buf);
        let header = LookupHeader::read(&mut cursor)?;
        let mut mmap = memmap2::MmapOptions::new().len(buf.len()).map_anon()?;
        mmap[..buf.len()].copy_from_slice(buf);
        let mmap = mmap.make_read_only()?;
        Ok(Self {
            mmap,
            header,
            pos: 0,
            last_value: 0,
        })
    }

    pub fn with_fallback<T: ValueLookupTable>(self, fallback: T) -> impl ValueLookupTable<Error = io::Error> {
        AndThenLookup::new(
            self,
            MapErrLookup::new(fallback, |err| {
                io::Error::other(format!("Lookup fallback error: {err}"))
            }),
        )
    }

    fn seek_to_value(&mut self, value: u64) -> io::Result<()> {
        // Seek to the position of the value. Value must be in range.
        assert!(self.header.is_in_range(value));
        let offset_val = value - self.header.min;
        self.pos = usize::try_from(offset_val)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Value offset too large"))?;
        Ok(())
    }

    fn read_next(&mut self) -> io::Result<Option<[u8; 32]>> {
        let mut buf = [0u8; 32];
        let start = self
            .pos
            .checked_mul(32)
            .and_then(|v| v.checked_add(LookupHeader::SIZE))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Position overflow when calculating read offset",
                )
            })?;
        let end = start.checked_add(32).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Position overflow when calculating read end offset",
            )
        })?;
        let data = self.mmap.get(start..end).ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(data);
        self.pos += 1;
        Ok(Some(buf))
    }

    /// Returns the supported value range of the lookup table
    pub fn range(&self) -> RangeInclusive<u64> {
        RangeInclusive::new(self.header.min, self.header.max)
    }
}

impl ValueLookupTable for MMapValueLookup {
    type Error = io::Error;

    fn lookup(&mut self, value: u64) -> Result<Option<[u8; 32]>, Self::Error> {
        if !self.header.is_in_range(value) {
            return Ok(None);
        }

        if value != self.last_value + 1 {
            self.seek_to_value(value)?;
        }
        self.last_value = value;

        self.read_next()
    }
}

#[cfg(test)]
mod tests {
    use rand::{rngs::OsRng, Rng};

    use super::*;

    fn generate_lookup_data(min: u64, max: u64) -> Vec<u8> {
        let mut data = Vec::with_capacity(LookupHeader::SIZE + 32 * (max - min + 1) as usize);
        LookupHeader::new(min, max).encode_into(&mut data).unwrap();
        for i in min..=max {
            let byte = i % u64::from(u8::MAX);
            data.extend_from_slice(&[byte as u8; 32]);
        }
        data
    }

    #[test]
    fn it_reads_the_header_correctly() {
        let lookup_data = generate_lookup_data(0, 10);
        let lookup = MMapValueLookup::from_buf(&lookup_data).unwrap();
        assert_eq!(lookup.range(), 0..=10);
    }

    #[test]
    fn it_reads_from_the_data_from_offset_start() {
        const START: u64 = 1024 * 1024;
        const END: u64 = 2 * 1024 * 1024;
        let lookup_data = generate_lookup_data(START, END);
        let mut lookup = MMapValueLookup::from_buf(&lookup_data).unwrap();
        assert!(lookup.lookup(START - 1).unwrap().is_none());
        for v in START..=END {
            let value = lookup.lookup(v).unwrap().unwrap();
            let byte = v % u64::from(u8::MAX);
            assert_eq!(value, [byte as u8; 32], "Failed at value {}", v);
        }
    }

    #[test]
    fn it_reads_from_the_data_file_until_end() {
        const NUM: u64 = (LookupHeader::SIZE + 11) as u64;
        let lookup_data = generate_lookup_data(0, NUM);
        let mut lookup = MMapValueLookup::from_buf(&lookup_data).unwrap();
        for v in 0..=NUM {
            let value = lookup.lookup(v).unwrap().unwrap();
            let byte = v % u64::from(u8::MAX);
            assert_eq!(value, [byte as u8; 32], "Failed at value {}", v);
        }
    }

    #[test]
    fn it_reads_non_sequential_values() {
        const NUM: u64 = 1000u64;
        let lookup_data = generate_lookup_data(0, NUM);
        let mut lookup = MMapValueLookup::from_buf(&lookup_data).unwrap();
        for _ in 0..=1000 {
            let v = OsRng.gen_range(0..=NUM);
            let value = lookup.lookup(v).unwrap().unwrap();
            let byte = v % u64::from(u8::MAX);
            assert_eq!(value, [byte as u8; 32], "Failed at value {}", v);
        }
    }
}
