//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use tari_ootle_wallet_crypto::{StealthCryptoApiError, StealthProofError};
use tari_template_lib_types::{Amount, UtxoAddress, crypto::PedersenCommitmentBytes};

#[derive(Debug, thiserror::Error)]
pub enum StealthProviderError {
    #[error("Crypto error generating proof: {0}")]
    StealthProofError(#[from] StealthProofError),
    #[error("Crypto error generating bullet proof: {0}")]
    CryptoApiError(#[from] StealthCryptoApiError),
    #[error("Invalid destination address: {details}")]
    InvalidDestinationAddress { details: String },
    #[error("Range proof error: {details}")]
    RangeProofError { details: String },
    #[error("Blocking task panicked: {details}")]
    SpawnBlockingPanic { details: String },
    #[error("Invalid input for spending: {0}")]
    InvalidInput(InvalidStealthInputError),
    #[error("Unexpected error: {details}")]
    UnexpectedError { details: String },
    #[error(
        "Unbalanced transfer: total input amount ({total_revealed_input} revealed + blinded inputs) does not equal \
         total outputs amount ({output_amount})"
    )]
    UnbalancedTransfer {
        total_revealed_input: Amount,
        output_amount: Amount,
    },
    #[error("Failed to decrypt input with commitment {commitment}: {details}")]
    DecryptionFailed {
        commitment: PedersenCommitmentBytes,
        details: String,
    },
    #[error("L1 burn claim ownership proof validation failed")]
    BurnClaimOwnershipProofInvalid,
    #[error("L1 burn claim fee ({max_fee}) is greater than or equal to the claimed amount ({claimed})")]
    BurnClaimFeeTooHigh { claimed: u64, max_fee: u64 },
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidStealthInputError {
    #[error("Input UTXO not found")]
    UtxoNotFound,
    #[error("Input UTXO {address} is frozen and cannot be spent")]
    UtxoIsFrozen { address: UtxoAddress },
    #[error("Input UTXO {address} is burnt and cannot be spent")]
    UtxoIsBurnt { address: UtxoAddress },
}
