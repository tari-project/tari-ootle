//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, ops::Deref};

use smallvec::SmallVec;

/// A **immutable** byte buffer that can be stack-allocated if the buffer is smaller than L or heap-allocated if it is
/// larger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmallBytes<const L: usize> {
    inner: SmallVec<u8, L>,
}

impl<const L: usize> SmallBytes<L> {
    pub const FIXED_SIZE: usize = L;

    pub const fn empty() -> Self {
        Self { inner: SmallVec::new() }
    }

    pub fn from_slice(slice: &[u8]) -> Self {
        let inner = SmallVec::from_slice(slice);
        Self { inner }
    }

    pub fn from_slices(slices: &[&[u8]]) -> Self {
        let len = slices.iter().map(|s| s.len()).sum::<usize>();
        let mut inner = SmallVec::with_capacity(len);
        for slice in slices {
            inner.extend_from_slice(slice);
        }
        Self { inner }
    }

    pub fn new_from_array<const SZ: usize>(buf: [u8; SZ]) -> Self {
        assert!(SZ <= L);
        let mut full_buf = [0; L];
        full_buf[..SZ].copy_from_slice(&buf);
        let inner = SmallVec::from_buf_and_len(full_buf, SZ);
        Self { inner }
    }

    pub const fn make_stack_buf() -> [u8; L] {
        [0; L]
    }

    pub fn new_stack(buf: [u8; L], length: usize) -> Self {
        let inner = SmallVec::from_buf_and_len(buf, length);
        Self { inner }
    }

    pub fn new_heap(v: Vec<u8>) -> Self {
        let inner = SmallVec::from_vec(v);
        Self { inner }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.inner.as_ref()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.inner.into_vec()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_stack(&self) -> bool {
        !self.is_heap()
    }

    pub fn is_heap(&self) -> bool {
        self.inner.spilled()
    }
}

impl<const L: usize> Deref for SmallBytes<L> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const L: usize> AsRef<[u8]> for SmallBytes<L> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const L: usize> From<[u8; L]> for SmallBytes<L> {
    fn from(b: [u8; L]) -> Self {
        SmallBytes::new_from_array(b)
    }
}

impl<const L: usize> From<Vec<u8>> for SmallBytes<L> {
    fn from(b: Vec<u8>) -> Self {
        SmallBytes::new_heap(b)
    }
}
impl<const L: usize> From<SmallBytes<L>> for Vec<u8> {
    fn from(value: SmallBytes<L>) -> Self {
        value.into_vec()
    }
}

impl<const L: usize> Display for SmallBytes<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            // Hex
            for b in self.as_slice() {
                write!(f, "{:02x}", b)?;
            }
        } else {
            // Try to print as UTF-8
            write!(f, "{}", String::from_utf8_lossy(self.as_slice()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_vs_stack() {
        let bytes = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let stack = SmallBytes::<32>::from_slice(bytes);
        assert_eq!(stack.len(), bytes.len());
        assert_eq!(stack.as_slice(), bytes);
        assert!(stack.is_stack());
        assert_eq!(stack.into_vec(), bytes.to_vec());

        let bytes = &[123; 64];
        let heap = SmallBytes::<32>::from_slice(bytes);
        assert_eq!(heap.len(), bytes.len());
        assert_eq!(heap.as_slice(), bytes);
        assert!(heap.is_heap());
        assert_eq!(heap.into_vec(), bytes.to_vec());
    }

    #[test]
    fn from_slices() {
        let a = &[1, 2, 3];
        let b = &[4, 5, 6];
        let c = &[7, 8, 9];
        let d = &[10, 11, 12];
        let e = &[13, 14, 15];
        let bytes = [a.as_slice(), b.as_slice(), c.as_slice(), d.as_slice(), e.as_slice()].concat();
        let stack = SmallBytes::<32>::from_slices(&[a, b, c, d, e]);
        assert_eq!(stack.len(), bytes.len());
        assert_eq!(stack.as_slice(), &bytes);
        assert!(stack.is_stack());
        assert_eq!(stack.as_ref(), bytes);
    }
}
