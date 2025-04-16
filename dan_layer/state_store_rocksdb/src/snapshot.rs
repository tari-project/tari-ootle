//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use rocksdb::{DBAccess, SnapshotWithThreadMode};

use crate::{
    cf_api::CfContext,
    error::RocksDbStorageError,
    traits::{Cf, RocksDatabase},
};

pub struct SnapshotContext<'db, DB: DBAccess> {
    db: &'db DB,
    snapshot: SnapshotWithThreadMode<'db, DB>,
}

impl<'db, DB: RocksDatabase + DBAccess> SnapshotContext<'db, DB> {
    pub(crate) fn new(db: &'db DB, snapshot: SnapshotWithThreadMode<'db, DB>) -> Self {
        Self { db, snapshot }
    }

    pub fn cf<CF: Cf>(
        &'db self,
        _cf: CF,
    ) -> Result<CfContext<'db, SnapshotWithThreadMode<'db, DB>, CF>, RocksDbStorageError> {
        let handle = self
            .db
            .cf_handle(CF::name())
            .ok_or_else(|| RocksDbStorageError::ColumnFamilyNotFound {
                operation: "create CF context",
                cf: format!("CF={}, cf_name={}", type_name::<CF>(), CF::name()),
            })?;
        CfContext::create(&self.snapshot, handle)
    }
}
