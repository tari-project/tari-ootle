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
    async fn refresh_account_with_source(
        &self,
        account: ComponentAddress,
        scan_for_utxos: bool,
        source: BalanceChangeSource,
    ) -> Result<bool, AccountMonitorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(AccountMonitorRequest::RefreshAccount {
                account,
                scan_for_utxos,
                source,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }

    /// Triggers an immediate refresh of the specified account. Returns `true` if the account was updated, otherwise
    /// `false`.
    pub async fn refresh_account(&self, account: ComponentAddress) -> Result<bool, AccountMonitorError> {
        self.refresh_account_with_source(account, false, BalanceChangeSource::Scan)
            .await
    }

    pub async fn refresh_account_with_utxos(&self, account: ComponentAddress) -> Result<bool, AccountMonitorError> {
        self.refresh_account_with_source(account, true, BalanceChangeSource::Scan)
            .await
    }

    pub(crate) async fn refresh_account_for_recovery(
        &self,
        account: ComponentAddress,
    ) -> Result<bool, AccountMonitorError> {
        self.refresh_account_with_source(account, true, BalanceChangeSource::Recovery)
            .await
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

#[cfg(test)]
mod tests {
    use std::{future::Future, str::FromStr};

    use super::*;

    async fn assert_refresh_source<F, Fut>(invoke: F, expected_source: BalanceChangeSource, expected_utxo_scan: bool)
    where
        F: FnOnce(AccountMonitorHandle, ComponentAddress) -> Fut,
        Fut: Future<Output = Result<bool, AccountMonitorError>>,
    {
        let account =
            ComponentAddress::from_str("component_91bef6af37bfb39b20260275c37a9e8acfc0517127284cd8f05944c8ffffffff")
                .unwrap();
        let (sender, mut receiver) = mpsc::channel(1);
        let handle = AccountMonitorHandle { sender };

        let invoke = invoke(handle, account);
        let receive = async {
            let AccountMonitorRequest::RefreshAccount {
                account: received_account,
                scan_for_utxos,
                source,
                reply,
            } = receiver.recv().await.unwrap()
            else {
                panic!("expected a refresh request");
            };
            assert_eq!(received_account, account);
            assert_eq!(scan_for_utxos, expected_utxo_scan);
            assert_eq!(source, expected_source);
            reply.send(Ok(true)).unwrap();
        };

        let (result, ()) = tokio::join!(invoke, receive);
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn refresh_requests_preserve_their_balance_change_source() {
        assert_refresh_source(
            |handle, account| async move { handle.refresh_account(account).await },
            BalanceChangeSource::Scan,
            false,
        )
        .await;
        assert_refresh_source(
            |handle, account| async move { handle.refresh_account_with_utxos(account).await },
            BalanceChangeSource::Scan,
            true,
        )
        .await;
        assert_refresh_source(
            |handle, account| async move { handle.refresh_account_for_recovery(account).await },
            BalanceChangeSource::Recovery,
            true,
        )
        .await;
    }
}
