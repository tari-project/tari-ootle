//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_crypto::StealthProofError;

use crate::{signer::SignerError, stealth::StealthProviderError, Address};

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("Signer not found for address {address}")]
    KeyProviderNotFound { address: Address },
    #[error(transparent)]
    SignerError(#[from] SignerError),
    #[error("Stealth proof error: {0}")]
    StealthProofError(#[from] StealthProofError),
    #[error("Stealth provider error: {0}")]
    StealthProviderError(#[from] StealthProviderError),
}
