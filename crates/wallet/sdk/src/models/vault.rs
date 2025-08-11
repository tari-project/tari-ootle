//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, VaultId},
    resource::ResourceType,
    types::Amount,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultModel {
    pub account_address: ComponentAddress,
    pub id: VaultId,
    pub resource_address: ResourceAddress,
    pub resource_type: ResourceType,
    pub confidential_balance: Amount,
    pub revealed_balance: Amount,
    pub locked_revealed_balance: Amount,
    pub token_symbol: Option<String>,
    pub divisibility: u8,
}

impl VaultModel {
    pub fn available_revealed_balance(&self) -> Amount {
        self.revealed_balance
            .checked_sub(self.locked_revealed_balance)
            .expect("Revealed balance should always be greater than or equal to locked revealed balance")
    }
}

#[derive(Debug, Clone)]
pub struct VaultBalance {
    pub account: ComponentAddress,
    pub confidential: Amount,
    pub revealed: Amount,
}
