//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    convert::TryFrom,
    fmt::{Display, Formatter},
    ops::Deref,
};

pub type ModelKey<const L: usize> = CompositeKey<L>;
pub type CfKey<const L: usize> = CompositeKey<L>;
#[derive(Debug, Clone)]
enum SmallBytes<const L: usize> {
    Fixed([u8; L]),
    Dynamic(Vec<u8>),
}

impl<const L: usize> SmallBytes<L> {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            SmallBytes::Fixed(b) => b.as_ref(),
            SmallBytes::Dynamic(b) => b.as_ref(),
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            SmallBytes::Fixed(b) => b.as_mut(),
            SmallBytes::Dynamic(b) => b.as_mut(),
        }
    }
}

impl<const L: usize> Deref for SmallBytes<L> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            SmallBytes::Fixed(b) => b,
            SmallBytes::Dynamic(b) => &**b,
        }
    }
}

impl AsRef<[u8]> for SmallBytes<32> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompositeKey<const L: usize> {
    bytes: SmallBytes<L>,
    len: usize,
}

impl<const L: usize> CompositeKey<L> {
    pub(super) fn new() -> Self {
        Self {
            bytes: Self::new_buf(),
            len: 0,
        }
    }

    pub fn from_parts<T: AsRef<[u8]>>(parts: &[T]) -> Self {
        Self::try_from_parts(parts).unwrap()
    }

    pub fn try_from_parts<T: AsRef<[u8]>>(parts: &[T]) -> Result<Self, CompositeKeyError> {
        let mut key = Self::new();
        for part in parts {
            if !key.try_push(part) {
                return Err(CompositeKeyError::LengthExceeded);
            }
        }
        Ok(key)
    }

    pub fn push<T: AsRef<[u8]>>(&mut self, bytes: T) -> &mut Self {
        if !self.try_push(bytes) {
            panic!("push: Composite key length exceeded");
        }
        self
    }

    pub fn try_push<T: AsRef<[u8]>>(&mut self, bytes: T) -> bool {
        let b = bytes.as_ref();
        let new_len = self.len + b.len();
        if new_len > L {
            return false;
        }
        self.bytes.as_mut_slice()[self.len..new_len].copy_from_slice(b);
        self.len = new_len;
        true
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes.as_slice()[..self.len]
    }

    pub fn to_be_u64(&self, offset: usize) -> Result<u64, CompositeKeyError> {
        if offset + 8 > self.len {
            return Err(CompositeKeyError::LengthExceeded);
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[offset..offset + 8]);
        Ok(u64::from_be_bytes(buf))
    }

    /// Returns a fixed 0-filled byte array.
    fn new_buf() -> SmallBytes<L> {
        if L <= 64 {
            return SmallBytes::Fixed([0u8; L]);
        }
        SmallBytes::Dynamic(Box::new([0x0u8; L]))
    }

    pub fn section_iter<const SECTIONS: usize>(&self, sections: [usize; SECTIONS]) -> SectionIter<'_, SECTIONS> {
        SectionIter {
            sections,
            current: 0,
            pointer: 0,
            slice: self.as_bytes(),
        }
    }
}

impl<const L: usize> Display for CompositeKey<L> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for b in self.as_bytes() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl<const L: usize> Deref for CompositeKey<L> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl<const L: usize> AsRef<[u8]> for CompositeKey<L> {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<const L: usize> TryFrom<&[u8]> for CompositeKey<L> {
    type Error = CompositeKeyError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() > L {
            return Err(CompositeKeyError::LengthExceeded);
        }
        let mut key = Self::new();
        key.bytes.as_mut_slice()[..value.len()].copy_from_slice(value);
        key.len = value.len();
        Ok(key)
    }
}

pub(super) struct SectionIter<'a, const SECTIONS: usize> {
    sections: [usize; SECTIONS],
    current: usize,
    pointer: usize,
    slice: &'a [u8],
}

impl<'a, const SECTIONS: usize> SectionIter<'a, SECTIONS> {
    /// Returns the next 8 bytes as a u64.
    ///
    /// # Panics
    /// Panics if the next section is not 8 bytes long.
    pub fn next_be_u64(&mut self) -> Option<u64> {
        let bytes = self.next()?;
        assert_eq!(
            bytes.len(),
            8,
            "Next section is not 8 bytes long. Section length: {}",
            bytes.len()
        );
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Some(u64::from_be_bytes(buf))
    }
}

impl<'a, const SECTIONS: usize> Iterator for SectionIter<'a, SECTIONS> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let cur_section = *self.sections.get(self.current)?;

        let lower = self.pointer;
        let upper = self.pointer + cur_section;
        self.current += 1;
        self.pointer = upper;

        if upper > self.slice.len() {
            return None;
        }

        Some(&self.slice[lower..upper])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CompositeKeyError {
    #[error("Composite key length exceeded")]
    LengthExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod section_iter {
        use super::*;

        #[test]
        fn it_returns_section_slices() {
            let key = CompositeKey::<10>::try_from_parts(&[&[1, 3][..], &[6], &[7, 8, 9]]).unwrap();
            let mut iter = key.section_iter([2, 1, 3, 0]);
            assert_eq!(iter.next(), Some(&[1, 3][..]));
            assert_eq!(iter.next(), Some(&[6][..]));
            assert_eq!(iter.next(), Some(&[7, 8, 9][..]));
            assert_eq!(iter.next(), Some(&[][..]));
            assert_eq!(iter.next(), None);
        }

        #[test]
        fn it_returns_none_if_sections_dont_exist() {
            let key = CompositeKey::<10>::try_from_parts(&[&[1, 3][..]]).unwrap();
            let mut iter = key.section_iter([2, 1, 3]);
            assert_eq!(iter.next(), Some(&[1, 3][..]));
            assert_eq!(iter.next(), None);
            assert_eq!(iter.next(), None);
            assert_eq!(iter.next(), None);
        }

        #[test]
        fn it_returns_none_for_less_sections_than_len() {
            let key = CompositeKey::<10>::try_from_parts(&[&[1, 3][..], &[1, 1, 1]]).unwrap();
            let mut iter = key.section_iter([3]);
            assert_eq!(iter.next(), Some(&[1, 3, 1][..]));
            assert_eq!(iter.next(), None);
            assert_eq!(iter.next(), None);
        }
    }
}
