//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{
    models::{AccountWithAddress, WalletEvent},
    WalletSdk,
    WalletSdkSpec,
};
use tari_template_lib::models::ResourceAddress;

use crate::{
    notify::Notify,
    utxo_scanner::{StealthScannerApiError, UtxoScanRoundStats, UtxoScannerRound},
};

pub struct UtxoScanner<TSpec: WalletSdkSpec> {
    sdk: WalletSdk<TSpec>,
    wallet_notify: Notify<WalletEvent>,
}

impl<TSpec: WalletSdkSpec> UtxoScanner<TSpec> {
    pub fn new(sdk: WalletSdk<TSpec>, wallet_notify: Notify<WalletEvent>) -> Self {
        Self { sdk, wallet_notify }
    }

    pub async fn scan_and_enqueue_utxos(
        &self,
        account: &AccountWithAddress,
        resource_address: &ResourceAddress,
    ) -> Result<UtxoScanRoundStats, StealthScannerApiError> {
        let network = self.sdk.config_api().get_network()?;

        let view_key = self.sdk.key_manager_api().get_key(account.view_only_key_id())?;

        let mut scanner_round = UtxoScannerRound::new(
            network,
            &self.sdk,
            account,
            &view_key,
            resource_address,
            &self.wallet_notify,
        );
        scanner_round.scan_for_utxo_updates().await?;

        Ok(scanner_round.into_stats())
    }
}
