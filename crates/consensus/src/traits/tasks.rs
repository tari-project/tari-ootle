//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{future::Future, time::Duration};

use log::info;
use tokio::task::AbortHandle;

use crate::tracing::TraceTimer;

const LOG_TARGET: &str = "tari::ootle::consensus::tasks";

pub(crate) trait PeriodicTask {
    fn name() -> &'static str;

    fn do_work_periodically(self, interval: Duration) -> AbortOnDropGuard
    where Self: Sized + Send + Sync + 'static {
        let handle = tokio::spawn(async move {
            info!(target: LOG_TARGET, "{} task starting periodically every {:.2?}", Self::name(), interval);
            loop {
                tokio::time::sleep(interval).await;
                info!(target: LOG_TARGET, "{} task starting", Self::name());
                {
                    let _timer =
                        TraceTimer::info(LOG_TARGET, Self::name()).with_excessive_threshold(Duration::from_secs(5));
                    self.do_work().await;
                }
                info!(target: LOG_TARGET, "{} task completed successfully", Self::name());
            }
        });

        AbortOnDropGuard::new(handle.abort_handle())
    }

    fn do_work(&self) -> impl Future<Output = ()> + Send;
}

pub(crate) struct AbortOnDropGuard {
    handle: AbortHandle,
}

impl AbortOnDropGuard {
    fn new(handle: AbortHandle) -> Self {
        Self { handle }
    }
}

impl Drop for AbortOnDropGuard {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
