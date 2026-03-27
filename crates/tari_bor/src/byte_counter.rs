//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone, Default)]
pub struct ByteCounter {
    count: usize,
    limit: Option<usize>,
}

impl ByteCounter {
    pub fn new() -> Self {
        Default::default()
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

#[cfg(not(feature = "std"))]
#[derive(Debug)]
pub struct ByteCounterError;

#[cfg(not(feature = "std"))]
impl ciborium_io::Write for ByteCounter {
    type Error = ByteCounterError;

    fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.count = self.count.checked_add(data.len()).ok_or(ByteCounterError)?;
        if let Some(limit) = self.limit {
            if self.count > limit {
                return Err(ByteCounterError);
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(feature = "std")]
impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len();
        self.count = self
            .count
            .checked_add(len)
            .ok_or_else(|| std::io::Error::other("ByteCounter overflow"))?;
        if let Some(limit) = self.limit &&
            self.count > limit
        {
            return Err(std::io::Error::other("ByteCounter limit exceeded"));
        }
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
