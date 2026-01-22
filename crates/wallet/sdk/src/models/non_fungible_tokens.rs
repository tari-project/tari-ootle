//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::{NonFungibleId, ResourceAddress, VaultId};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NonFungibleToken {
    pub vault_id: VaultId,
    pub nft_id: NonFungibleId,
    pub resource_address: ResourceAddress,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    #[serde(with = "ootle_serde::cbor_value")]
    pub data: tari_bor::Value,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    #[serde(with = "ootle_serde::cbor_value")]
    pub mutable_data: tari_bor::Value,
    pub is_burnt: bool,
}
