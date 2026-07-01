//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Transaction signing traits and ephemeral signers.
//!
//! Defines the signing interfaces used by wallets and key providers:
//!
//! - [`TransactionSigner`] — signs and authorizes transactions with a persistent key.
//! - [`TransactionSealSigner`] — applies the final seal signature to a transaction.
//! - [`TransactionStealthKeySigner`] — signs using derived stealth keys for confidential transactions.

pub mod adaptor;
pub mod ephemeral_signer;
mod signer;

pub use signer::*;
