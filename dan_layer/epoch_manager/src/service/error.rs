//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage_sqlite::error::SqliteStorageError;

use crate::EpochManagerError;

impl From<SqliteStorageError> for EpochManagerError {
    fn from(e: SqliteStorageError) -> Self {
        Self::StorageError(e.into())
    }
}
