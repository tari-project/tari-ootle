//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_engine_types::substate::{Substate, SubstateId, SubstateValue};

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
            SubstateType::ValidatorFeePool => "vnfp",
            SubstateType::Template => "template",
        }
    }

    pub fn matches(&self, addr: &SubstateId) -> bool {
        #[allow(clippy::match_like_matches_macro)]
        match (self, addr) {
            (SubstateType::Component, SubstateId::Component(_)) => true,
            (SubstateType::Resource, SubstateId::Resource(_)) => true,
            (SubstateType::Vault, SubstateId::Vault(_)) => true,
            (SubstateType::NonFungible, SubstateId::NonFungible(_)) => true,
            (SubstateType::UnclaimedConfidentialOutput, SubstateId::UnclaimedConfidentialOutput(_)) => true,
            (SubstateType::TransactionReceipt, SubstateId::TransactionReceipt(_)) => true,
            (SubstateType::ValidatorFeePool, SubstateId::ValidatorFeePool(_)) => true,
            (SubstateType::Template, SubstateId::Template(_)) => true,
            _ => false,
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
            SubstateValue::Template(_) => SubstateType::Template,
            SubstateValue::ValidatorFeePool(_) => SubstateType::ValidatorFeePool,
        }
    }
}

impl From<&SubstateId> for SubstateType {
    fn from(value: &SubstateId) -> Self {
        match value {
            SubstateId::Component(_) => SubstateType::Component,
            SubstateId::Resource(_) => SubstateType::Resource,
            SubstateId::Vault(_) => SubstateType::Vault,
            SubstateId::UnclaimedConfidentialOutput(_) => SubstateType::UnclaimedConfidentialOutput,
            SubstateId::NonFungible(_) => SubstateType::NonFungible,
            SubstateId::TransactionReceipt(_) => SubstateType::TransactionReceipt,
            SubstateId::ValidatorFeePool(_) => SubstateType::ValidatorFeePool,
            SubstateId::Template(_) => SubstateType::Template,
        }
    }
}

impl From<&Substate> for SubstateType {
    fn from(value: &Substate) -> Self {
        value.substate_value().into()
    }
}

impl Display for SubstateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_prefix_str())
    }
}
