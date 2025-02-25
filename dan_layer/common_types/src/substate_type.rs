//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_engine_types::substate::SubstateValue;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum SubstateType {
    Component,
    Resource,
    Vault,
    UnclaimedConfidentialOutput,
    NonFungible,
    TransactionReceipt,
    NonFungibleIndex,
    ValidatorFeePool,
    Template,
}

impl SubstateType {
    pub fn as_prefix_str(&self) -> &str {
        match self {
            SubstateType::Component => "component",
            SubstateType::Resource => "resource",
            SubstateType::Vault => "vault",
            SubstateType::UnclaimedConfidentialOutput => "commitment",
            SubstateType::NonFungible => "nft",
            SubstateType::TransactionReceipt => "txreceipt",
            SubstateType::NonFungibleIndex => "nftindex",
            SubstateType::ValidatorFeePool => "vnfp",
            SubstateType::Template => "template",
        }
    }
}

impl From<&SubstateValue> for SubstateType {
    fn from(value: &SubstateValue) -> Self {
        match value {
            SubstateValue::Component(_) => SubstateType::Component,
            SubstateValue::Resource(_) => SubstateType::Resource,
            SubstateValue::Vault(_) => SubstateType::Vault,
            SubstateValue::UnclaimedConfidentialOutput(_) => SubstateType::UnclaimedConfidentialOutput,
            SubstateValue::NonFungible(_) => SubstateType::NonFungible,
            SubstateValue::TransactionReceipt(_) => SubstateType::TransactionReceipt,
            SubstateValue::NonFungibleIndex(_) => SubstateType::NonFungibleIndex,
            SubstateValue::Template(_) => SubstateType::Template,
            SubstateValue::ValidatorFeePool(_) => SubstateType::ValidatorFeePool,
        }
    }
}

impl Display for SubstateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_prefix_str())
    }
}
