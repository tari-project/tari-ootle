//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_transport::APDUErrorCode;
use ootle_ledger_common::OotleStatusWord;

/// Error returned by `LedgerClient` operations; `E` is the transport's error type.
#[derive(Debug, thiserror::Error)]
pub enum LedgerClientError<E> {
    /// The underlying APDU transport failed (USB HID, Speculos HTTP, etc.).
    #[error("Ledger transport error: {0}")]
    Transport(#[from] E),
    /// The device replied OK but the response body failed to decode.
    #[error("Invalid response from ledger: {details}")]
    InvalidResponse { details: String },
    /// The device returned a standard (ISO 7816 / Ledger SDK) error status word.
    #[error("Ledger returned APDU error code: {code}")]
    APDUError { code: APDUErrorCode },
    /// The device returned a status word that is neither a standard APDU code nor an
    /// [`OotleStatusWord`].
    #[error("Ledger returned unknown APDU error code: {code}")]
    APDUOtherCodeError { code: u16 },
    /// The Ootle app returned an app-specific error, e.g. the user rejected the transaction.
    #[error("Ledger returned application error code: {code:?}")]
    AppError { code: OotleStatusWord },
}
