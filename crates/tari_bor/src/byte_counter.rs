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

impl ByteCounter {
    fn add(&mut self, n: usize) -> Result<(), ByteCounterError> {
        self.count = self
            .count
            .checked_add(n)
            .ok_or(ByteCounterError("ByteCounter overflow"))?;
        if let Some(limit) = self.limit &&
            self.count > limit
        {
            return Err(ByteCounterError("ByteCounter limit exceeded"));
        }
        Ok(())
    }
}

impl minicbor::encode::Write for ByteCounter {
    type Error = ByteCounterError;

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.add(buf.len())
    }
}

/// Lets `ByteCounter` stand in for any `std::io::Write` sink — useful when host-side code calls
/// [`crate::encode_into_writer`] purely to measure the encoded length.
#[cfg(feature = "std")]
impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.add(buf.len()).map_err(|e| std::io::Error::other(e.0))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
