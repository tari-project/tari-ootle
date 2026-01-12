//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use digest::consts;
use jmt::SimpleHasher;
use tari_hashing::layer2::{tari_hasher32, TariDomainHasher};

use crate::key_mapper::JmtSubstateHashDomain;

type JmtHasher = TariDomainHasher<JmtSubstateHashDomain, consts::U32>;

pub struct OotleJmtHasher {
    hasher: JmtHasher,
}

impl SimpleHasher for OotleJmtHasher {
    fn new() -> Self {
        Self {
            hasher: tari_hasher32("StateTreeNode"),
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.hasher.update_consensus_encode(data);
    }

    fn finalize(self) -> [u8; 32] {
        self.hasher.finalize_into_array()
    }
}
