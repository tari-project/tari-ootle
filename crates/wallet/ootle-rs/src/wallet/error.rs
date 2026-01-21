//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{signer::SignerError, Address};

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("Signer not found for address {address}")]
    SignerNotFound { address: Address },
    #[error(transparent)]
    SignerError(#[from] SignerError),
}
