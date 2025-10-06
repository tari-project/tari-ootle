//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_wallet_sdk::{
    models::AccountWithAddress,
    network::{StatusResponseError, WalletNetworkInterface},
    storage::WalletStore,
    WalletSdk,
};
use tari_template_lib::models::ResourceAddress;

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

    pub async fn scan_and_enqueue_utxos(
        &self,
        account: &AccountWithAddress,
        resource_address: &ResourceAddress,
    ) -> Result<usize, StealthScannerApiError> {
        let network = self.sdk.config_api().get_network()?;

        let view_key = self
            .sdk
            .key_manager_api()
            .get_view_only_key(account.view_only_key_id())?;

        let mut scanner_round = UtxoScannerRound::new(network, &self.sdk, account, &view_key, resource_address);
        let num_found = scanner_round.scan_for_utxo_updates().await?;

        Ok(num_found)
    }
}
