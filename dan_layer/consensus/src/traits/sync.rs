//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

pub trait SyncManager {
    type Error: std::error::Error + Send + Sync + 'static;

    fn check_sync(&self) -> impl Future<Output = Result<SyncStatus, Self::Error>> + Send;

    fn sync(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    UpToDate,
    Behind,
}
