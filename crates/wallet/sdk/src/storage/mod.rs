//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub use error::*;
pub use reader::*;
pub use writer::*;

mod error;
mod reader;
mod writer;

use std::ops::{Deref, DerefMut};

pub trait ReadableWalletStore {
    type ReadTransaction<'a>: WalletStoreReader
    where Self: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, WalletStorageError>;

    fn with_read_tx<F: FnOnce(&mut Self::ReadTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<WalletStorageError> {
        let mut tx = self.create_read_tx()?;
        let ret = f(&mut tx)?;
        Ok(ret)
    }
}

impl<T: ReadableWalletStore> ReadableWalletStore for &T {
    type ReadTransaction<'a>
        = T::ReadTransaction<'a>
    where Self: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, WalletStorageError> {
        (**self).create_read_tx()
    }
}

pub trait WriteableWalletStore: ReadableWalletStore {
    type WriteTransaction<'a>: WalletStoreWriter + Deref<Target = Self::ReadTransaction<'a>> + DerefMut
    where Self: 'a;

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, WalletStorageError>;

    fn with_write_tx<F: FnOnce(&mut Self::WriteTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<WalletStorageError> {
        let mut tx = self.create_write_tx()?;
        match f(&mut tx) {
            Ok(r) => {
                tx.commit()?;
                Ok(r)
            },
            Err(e) => {
                if let Err(err) = tx.rollback() {
                    log::error!("Failed to rollback transaction: {}", err);
                }
                Err(e)
            },
        }
    }
}

impl<T: WriteableWalletStore> WriteableWalletStore for &T {
    type WriteTransaction<'a>
        = T::WriteTransaction<'a>
    where Self: 'a;

    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, WalletStorageError> {
        (**self).create_write_tx()
    }
}

pub trait WalletStore: ReadableWalletStore + WriteableWalletStore {}

impl<T> WalletStore for T where T: ReadableWalletStore + WriteableWalletStore {}
