//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_common_types::seeds::{cipher_seed::CipherSeed, seed_words::SeedWords};

#[derive(Debug, Copy, Clone, Default)]
pub enum CipherSeedRestore<'a> {
    #[default]
    CreateNewIfRequired,
    FromSeedWords(&'a SeedWords),
}

impl<'a> CipherSeedRestore<'a> {
    pub fn is_create_new(&self) -> bool {
        matches!(self, CipherSeedRestore::CreateNewIfRequired)
    }
}

pub type SafeCipherSeed = Arc<CipherSeed>;

#[derive(Debug, Clone, Default)]
pub enum WalletCipherSeed {
    #[default]
    None,
    CipherSeed(SafeCipherSeed),
}

impl WalletCipherSeed {
    pub fn cipher_seed(&self) -> Option<&SafeCipherSeed> {
        match self {
            Self::CipherSeed(seed) => Some(seed),
            Self::None => None,
        }
    }
}

impl From<CipherSeed> for WalletCipherSeed {
    fn from(seed: CipherSeed) -> Self {
        Self::CipherSeed(Arc::new(seed))
    }
}
