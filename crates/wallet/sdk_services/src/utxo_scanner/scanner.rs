//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_wallet_sdk::{
    models::AccountWithPublicKey,
    network::{StatusResponseError, WalletNetworkInterface},
    storage::WalletStore,
    WalletSdk,
};
use tari_template_lib::models::ResourceAddress;
use tokio::sync::watch;

use crate::utxo_scanner::{StealthScannerApiError, UtxoScannerRound};

pub struct UtxoScanner<TStore, TWalletInterface> {
    sdk: WalletSdk<TStore, TWalletInterface>,
}

impl<TStore, TNetworkInterface> UtxoScanner<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub fn new(sdk: WalletSdk<TStore, TNetworkInterface>) -> Self {
        Self { sdk }
    }

    pub async fn scan_and_recover_utxos(
        &self,
        account: &AccountWithPublicKey,
        resource_address: &ResourceAddress,
        notify_tx: &watch::Sender<()>,
    ) -> Result<(), StealthScannerApiError> {
        let network = self.sdk.config_api().get_network()?;

        let account_key = self.sdk.key_manager_api().derive_account_key(account.key_index())?;

        let mut scanner_round =
            UtxoScannerRound::new(network, &self.sdk, notify_tx, account, &account_key, resource_address);
        scanner_round.scan_for_utxo_updates().await?;

        Ok(())
    }
}
