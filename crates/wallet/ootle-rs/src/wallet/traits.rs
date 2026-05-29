//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use tari_ootle_transaction::{Transaction, UnsignedTransaction};

use crate::{
    Address,
    stealth::{BurnClaimKeyProvider, InputDecryptor, StealthOutputStatementFactory},
    transaction::{TransactionSigner, TransactionStealthKeySigner},
    wallet::WalletResult,
};

/// Trait for wallets that can sign transactions on a specific network.
pub trait NetworkWallet {
    fn default_address(&self) -> &Address;

    fn sign_transaction(&self, unsigned: UnsignedTransaction)
    -> impl Future<Output = WalletResult<Transaction>> + Send;
}

/// A key provider that can sign transactions, derive stealth keys, generate output
/// statements, decrypt stealth inputs, and claim Layer 1 burns. Automatically implemented
/// for any type implementing all constituent traits.
pub trait WalletKeyProvider:
    TransactionSigner + TransactionStealthKeySigner + StealthOutputStatementFactory + InputDecryptor + BurnClaimKeyProvider
{
}

impl<T> WalletKeyProvider for T where T: TransactionSigner
        + TransactionStealthKeySigner
        + StealthOutputStatementFactory
        + InputDecryptor
        + BurnClaimKeyProvider
{
}
