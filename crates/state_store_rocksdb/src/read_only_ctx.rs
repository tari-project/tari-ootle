//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use crate::{cf_api::CfContext, dbs::read_only::ReadOnlyDb, error::RocksDbStorageError, traits::Cf};

pub struct ReadOnlyContext<'db> {
    db: &'db ReadOnlyDb,
}

impl<'db> ReadOnlyContext<'db> {
    pub(crate) fn new(db: &'db ReadOnlyDb) -> Self {
        Self { db }
    }

    pub fn cf<CF: Cf>(&'db self, _cf: CF) -> Result<CfContext<'db, ReadOnlyDb, CF>, RocksDbStorageError> {
        let handle = self
            .db
            .cf_handle(CF::name())
            .ok_or_else(|| RocksDbStorageError::ColumnFamilyNotFound {
                operation: "create CF context",
                cf: format!("CF={}, cf_name={}", type_name::<CF>(), CF::name()),
            })?;
        CfContext::create(self.db, handle)
    }
}
