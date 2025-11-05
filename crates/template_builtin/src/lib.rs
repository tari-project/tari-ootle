//  Copyright 2022, The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_template_lib::types::TemplateAddress;

/// Address of the account template.
/// 0000000000000000000000000000000000000000000000000000000000000000
pub const ACCOUNT_TEMPLATE_ADDRESS: TemplateAddress = TemplateAddress::from_array([0; 32]);
/// Address of the NFT faucet template.
/// 0000000000000000000000000000000000000000000000000000000000000001
pub const NFT_FAUCET_TEMPLATE_ADDRESS: TemplateAddress = TemplateAddress::from_array([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]);
/// Address of the XTR faucet template.
/// 0102030000000000000000000000000000000000000000000000000000000000
pub const XTR_FAUCET_TEMPLATE_ADDRESS: TemplateAddress = TemplateAddress::from_array([
    1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]);

pub const LIQUIDITY_POOL_TEMPLATE_ADDRESS: TemplateAddress = TemplateAddress::from_array([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
]);

pub fn get_template_builtin(address: &TemplateAddress) -> &'static [u8] {
    try_get_template_builtin(address).unwrap_or_else(|| panic!("Unknown builtin template address {address}"))
}

pub fn try_get_template_builtin(address: &TemplateAddress) -> Option<&'static [u8]> {
    all_builtin_templates().find(|(a, _)| a == address).map(|(_, b)| b)
}

pub fn all_builtin_templates() -> impl Iterator<Item = (TemplateAddress, &'static [u8])> {
    [
        (
            ACCOUNT_TEMPLATE_ADDRESS,
            include_bytes!("../templates/account/account.wasm").as_slice(),
        ),
        (
            NFT_FAUCET_TEMPLATE_ADDRESS,
            include_bytes!("../templates/nft_faucet/nft_faucet.wasm").as_slice(),
        ),
        (
            XTR_FAUCET_TEMPLATE_ADDRESS,
            include_bytes!("../templates/faucet/faucet.wasm").as_slice(),
        ),
        // TODO: Uncomment when the liquidity pool template is ready
        // (
        //     LIQUIDITY_POOL_TEMPLATE_ADDRESS,
        //     include_bytes!("../templates/liquidity_pool/liquidity_pool.wasm").as_slice(),
        // ),
    ]
    .into_iter()
}
