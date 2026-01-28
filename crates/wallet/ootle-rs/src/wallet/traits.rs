//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use tari_ootle_transaction::{Transaction, UnsignedTransaction};

use crate::{
    stealth::{InputDecryptor, StealthOutputStatementFactory},
    transaction::{TransactionSigner, TransactionStealthKeySigner},
    wallet::WalletResult,
    Address,
};

pub trait NetworkWallet {
    fn default_address(&self) -> &Address;

    fn sign_transaction(&self, unsigned: UnsignedTransaction)
        -> impl Future<Output = WalletResult<Transaction>> + Send;
}

pub trait WalletKeyProvider:
    TransactionSigner + TransactionStealthKeySigner + StealthOutputStatementFactory + InputDecryptor
{
}

impl<T> WalletKeyProvider for T where T: TransactionSigner + TransactionStealthKeySigner + StealthOutputStatementFactory + InputDecryptor
{}
