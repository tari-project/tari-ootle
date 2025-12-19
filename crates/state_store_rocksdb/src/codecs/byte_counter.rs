//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io;

#[derive(Debug, Clone, Default)]
pub struct ByteCounter {
    count: usize,
}

impl ByteCounter {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn get(&self) -> usize {
        self.count
    }
}

impl io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.count = self
            .count
            .checked_add(buf.len())
            .ok_or_else(|| io::Error::other("byte count overflow"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
