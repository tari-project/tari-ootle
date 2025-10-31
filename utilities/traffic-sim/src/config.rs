//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use reqwest::Url;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub exchange_wallet_url: String,
    pub wallets: Vec<WalletConfig>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WalletConfig {
    pub name: String,
    pub url: Url,
}
