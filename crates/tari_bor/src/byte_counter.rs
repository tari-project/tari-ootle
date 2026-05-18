//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// A [`minicbor::encode::Write`] implementation that counts written bytes instead of
/// storing them. Used to pre-calculate encoded length without allocating a buffer.
#[derive(Debug, Clone, Default)]
pub struct ByteCounter {
    count: usize,
    limit: Option<usize>,
}

impl ByteCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            count: 0,
            limit: Some(limit),
        }
    }

    pub fn get(&self) -> usize {
        self.count
    }
}

#[derive(Debug)]
pub struct ByteCounterError(&'static str);

impl core::fmt::Display for ByteCounterError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ByteCounterError {}

impl minicbor::encode::Write for ByteCounter {
    type Error = ByteCounterError;

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.count = self
            .count
            .checked_add(buf.len())
            .ok_or(ByteCounterError("ByteCounter overflow"))?;
        if let Some(limit) = self.limit &&
            self.count > limit
        {
            return Err(ByteCounterError("ByteCounter limit exceeded"));
        }
        Ok(())
    }
}
