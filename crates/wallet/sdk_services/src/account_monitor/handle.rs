//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::models::BalanceChangeSource;
use tari_template_lib_types::{ComponentAddress, ResourceAddress};
use tokio::sync::{mpsc, oneshot};

use crate::{Reply, account_monitor::monitor::AccountMonitorError};

#[derive(Debug)]
pub(super) enum AccountMonitorRequest {
    RefreshAccount {
        account: ComponentAddress,
        scan_for_utxos: bool,
        source: BalanceChangeSource,
        reply: Reply<Result<bool, AccountMonitorError>>,
    },
    AssociateResource {
        account: ComponentAddress,
        resource: ResourceAddress,
        reply: Reply<Result<(), AccountMonitorError>>,
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
                source: BalanceChangeSource::Scan,
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
                source: BalanceChangeSource::Scan,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }

    pub async fn recover_account(&self, account: ComponentAddress) -> Result<bool, AccountMonitorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(AccountMonitorRequest::RefreshAccount {
                account,
                scan_for_utxos: true,
                source: BalanceChangeSource::Recovery,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }

    pub async fn associate_resource(
        &self,
        account: ComponentAddress,
        resource: ResourceAddress,
    ) -> Result<(), AccountMonitorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(AccountMonitorRequest::AssociateResource {
                account,
                resource,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }
}
