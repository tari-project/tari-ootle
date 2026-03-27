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

pub type ArcCipherSeed = Arc<CipherSeed>;

#[derive(Debug, Clone, Default)]
pub enum WalletCipherSeed {
    #[default]
    None,
    CipherSeed(ArcCipherSeed),
}

impl WalletCipherSeed {
    pub fn cipher_seed(&self) -> Option<&CipherSeed> {
        match self {
            Self::CipherSeed(seed) => Some(seed),
            Self::None => None,
        }
    }

    pub fn into_cipher_seed(self) -> Option<ArcCipherSeed> {
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
