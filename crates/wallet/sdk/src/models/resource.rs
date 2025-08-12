//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{
    models::{Metadata, ResourceAddress},
    prelude::{AccessRules, Amount},
    resource::ResourceType,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResourceModel {
    pub address: ResourceAddress,
    pub resource_type: ResourceType,
    pub token_symbol: Option<String>,
    pub divisibility: u8,
    pub metadata: Metadata,
    pub access_rules: AccessRules,
    pub total_supply: Option<Amount>,
}
