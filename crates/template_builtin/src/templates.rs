//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::TemplateAddress;

use crate::{
    ACCOUNT_TEMPLATE_ADDRESS,
    LIQUIDITY_POOL_TEMPLATE_ADDRESS,
    NFT_FAUCET_TEMPLATE_ADDRESS,
    XTR_FAUCET_TEMPLATE_ADDRESS,
};

pub fn get_template_builtin(address: &TemplateAddress) -> &'static [u8] {
    try_get_template_builtin(address).unwrap_or_else(|| panic!("Unknown builtin template address {address}"))
}

pub fn try_get_template_builtin(address: &TemplateAddress) -> Option<&'static [u8]> {
    all_builtin_templates()
        .iter()
        .find(|(a, _)| a == address)
        .map(|(_, b)| *b)
}

pub const fn all_builtin_templates() -> &'static [(TemplateAddress, &'static [u8])] {
    &[
        (ACCOUNT_TEMPLATE_ADDRESS, include_bytes!("../compiled/account.wasm")),
        (
            NFT_FAUCET_TEMPLATE_ADDRESS,
            include_bytes!("../compiled/nft_faucet.wasm"),
        ),
        (XTR_FAUCET_TEMPLATE_ADDRESS, include_bytes!("../compiled/faucet.wasm")),
        (
            LIQUIDITY_POOL_TEMPLATE_ADDRESS,
            include_bytes!("../compiled/liquidity_pool.wasm"),
        ),
    ]
}
