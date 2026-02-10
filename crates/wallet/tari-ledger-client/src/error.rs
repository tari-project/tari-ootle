//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_transport::APDUErrorCode;
use minotari_ledger_wallet_common::common_types::AppSW;

#[derive(Debug, thiserror::Error)]
pub enum LedgerClientError<E> {
    #[error("Ledger transport error: {0}")]
    Transport(#[from] E),
    #[error("Invalid response from ledger: {details}")]
    InvalidResponse { details: String },
    #[error("Ledger returned APDU error code: {code}")]
    APDUError { code: APDUErrorCode },
    #[error("Ledger returned unknown APDU error code: {code}")]
    APDUOtherCodeError { code: u16 },
    #[error("Ledger returned application error code: {code:?}")]
    AppError { code: AppSW },
}
