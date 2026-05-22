//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    hash::{BuildHasher, Hasher},
};

use siphasher::sip::SipHasher13;

pub type UniqueSet<T> = HashSet<T, FixedState>;

#[derive(Debug, Clone, Default)]
struct FixedState;

impl BuildHasher for FixedState {
    type Hasher = SipHasher13;

    fn build_hasher(&self) -> SipHasher13 {
        SipHasher13::new_with_keys(0, 0)
    }
}

