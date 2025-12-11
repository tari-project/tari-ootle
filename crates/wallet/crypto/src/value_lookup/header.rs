//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, mem::size_of};

pub const LOOKUP_HEADER_LEADING_BYTES: &[u8] = b"VLKP";

pub struct LookupHeader {
    pub min: u64,
    pub max: u64,
}

impl LookupHeader {
    pub const SIZE: usize = size_of::<u64>() * 2 + LOOKUP_HEADER_LEADING_BYTES.len();

    pub fn new(min: u64, max: u64) -> Self {
        Self { min, max }
    }

    pub fn read<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let mut buf = [0u8; Self::SIZE];
        reader.read_exact(&mut buf)?;
        Self::from_buf(&buf)
    }

    pub fn from_buf(buf: &[u8]) -> io::Result<Self> {
        if &buf[0..4] != LOOKUP_HEADER_LEADING_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid lookup table header leading bytes",
            ));
        }
        let body = buf
            .get(LOOKUP_HEADER_LEADING_BYTES.len()..)
            .expect("len is at least LOOKUP_HEADER_LEADING_BYTES");
        if body.len() < size_of::<u64>() * 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid lookup table header body length, no min/max",
            ));
        }
        let mut u64_buf = [0u8; 8];
        u64_buf.copy_from_slice(body.get(..8).expect("slice length checked before"));
        let min = u64::from_be_bytes(u64_buf);
        let mut u64_buf = [0u8; 8];
        u64_buf.copy_from_slice(body.get(8..16).expect("slice length checked before"));
        let max = u64::from_be_bytes(u64_buf);
        Ok(Self { min, max })
    }

    pub fn encode_into<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        // Write header VLKP || min_value (8 bytes) || max_value (8 bytes)
        writer.write_all(LOOKUP_HEADER_LEADING_BYTES)?;
        writer.write_all(&self.min.to_be_bytes())?;
        writer.write_all(&self.max.to_be_bytes())?;
        Ok(())
    }

    pub fn is_in_range(&self, value: u64) -> bool {
        value >= self.min && value <= self.max
    }
}
