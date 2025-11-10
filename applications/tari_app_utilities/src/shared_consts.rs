//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::Amount;

/// The number of tokens initially minted by the testnet faucet.
/// Just under 18.5 trillion.
pub const TXTR_FAUCET_INITIAL_SUPPLY: Amount = Amount::from_u64(u64::MAX);
