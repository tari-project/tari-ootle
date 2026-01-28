//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct SafeAeadKey<const N: usize>(Box<[u8; N]>);

impl<const N: usize> AsRef<[u8]> for SafeAeadKey<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl<const N: usize> AsMut<[u8]> for SafeAeadKey<N> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0[..]
    }
}

impl<const N: usize> Default for SafeAeadKey<N> {
    fn default() -> Self {
        Self(Box::new([0u8; N]))
    }
}
