//  Copyright 2024, The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{cmp, convert::TryFrom, ops::Deref};

use borsh::BorshSerialize;
use bounded_vec::{witnesses, BoundedVec};
use serde::{Deserialize, Serialize};

type BoundedByteVec<const MAX: usize> = BoundedVec<u8, 0, MAX, witnesses::Empty<MAX>>;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub struct MaxSizeBytes<const MAX: usize> {
    inner: BoundedByteVec<MAX>,
}

impl<const MAX: usize> MaxSizeBytes<MAX> {
    pub fn into_vec(self) -> Vec<u8> {
        // NOTE: to_vec consumes the instance, BoundedVec does not use the correct naming i.e. into_vec.
        self.inner.to_vec()
    }

    pub fn from_vec(vec: Vec<u8>) -> Result<Self, MaxSizeBytesError> {
        if vec.len() > MAX {
            Err(MaxSizeBytesError::MaxSizeBytesLengthError {
                expected: MAX,
                actual: vec.len(),
            })
        } else {
            Ok(Self {
                inner: BoundedByteVec::from_vec(vec).expect("len <= MAX"),
            })
        }
    }

    pub fn from_bytes_checked<T: AsRef<[u8]>>(bytes: T) -> Option<Self> {
        let b = bytes.as_ref();
        if b.len() > MAX {
            None
        } else {
            Some(Self {
                inner: BoundedByteVec::from_vec(b.to_vec()).expect("len <= MAX"),
            })
        }
    }

    pub fn from_bytes_truncate<T: AsRef<[u8]>>(bytes: T) -> Self {
        let b = bytes.as_ref();
        let len = cmp::min(b.len(), MAX);
        Self {
            inner: BoundedByteVec::from_vec(b.get(..len).expect("len <= bytes.len()").to_vec()).expect("len <= MAX"),
        }
    }
}

impl<const MAX: usize> From<MaxSizeBytes<MAX>> for Vec<u8> {
    fn from(value: MaxSizeBytes<MAX>) -> Self {
        value.into_vec()
    }
}

impl<const MAX: usize> TryFrom<Vec<u8>> for MaxSizeBytes<MAX> {
    type Error = MaxSizeBytesError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        MaxSizeBytes::from_vec(value)
    }
}

impl<const MAX: usize> TryFrom<&[u8]> for MaxSizeBytes<MAX> {
    type Error = MaxSizeBytesError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        MaxSizeBytes::from_bytes_checked(value).ok_or(MaxSizeBytesError::MaxSizeBytesLengthError {
            expected: MAX,
            actual: value.len(),
        })
    }
}

impl<const MAX: usize> AsRef<[u8]> for MaxSizeBytes<MAX> {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_slice()
    }
}

impl<const MAX: usize> Deref for MaxSizeBytes<MAX> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.inner.as_slice()
    }
}

impl<const MAX: usize> BorshSerialize for MaxSizeBytes<MAX> {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(self.inner.as_slice(), writer)
    }
}

impl<const MAX: usize> PartialOrd for MaxSizeBytes<MAX> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<const MAX: usize> Ord for MaxSizeBytes<MAX> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.inner.as_slice().cmp(other.inner.as_slice())
    }
}

impl<const MAX: usize> Default for MaxSizeBytes<MAX> {
    fn default() -> Self {
        Self::from_vec(vec![]).expect("0 <= MAX")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MaxSizeBytesError {
    #[error("Invalid Bytes length: expected {expected}, got {actual}")]
    MaxSizeBytesLengthError { expected: usize, actual: usize },
}
