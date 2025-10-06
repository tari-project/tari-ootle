//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::models::ComponentAddress;
use tokio::sync::{mpsc, oneshot};

use crate::{account_monitor::monitor::AccountMonitorError, Reply};

#[derive(Debug)]
pub(super) enum AccountMonitorRequest {
    RefreshAccount {
        account: ComponentAddress,
        scan_for_utxos: bool,
        reply: Reply<Result<bool, AccountMonitorError>>,
    },
}

#[derive(Debug, Clone)]
pub struct AccountMonitorHandle {
    pub(super) sender: mpsc::Sender<AccountMonitorRequest>,
}

impl AccountMonitorHandle {
    /// Triggers an immediate refresh of the specified account. Returns `true` if the account was updated, otherwise
    /// `false`.
    pub async fn refresh_account(&self, account: ComponentAddress) -> Result<bool, AccountMonitorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(AccountMonitorRequest::RefreshAccount {
                account,
                scan_for_utxos: false,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }

    pub async fn refresh_account_with_utxos(&self, account: ComponentAddress) -> Result<bool, AccountMonitorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(AccountMonitorRequest::RefreshAccount {
                account,
                scan_for_utxos: true,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }
}
