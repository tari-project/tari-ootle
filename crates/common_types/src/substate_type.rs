//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_engine_types::substate::{Substate, SubstateDiff, SubstateId, SubstateValue};
use tari_template_lib_types::address_prefixes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SubstateType {
    Component,
    Resource,
    Vault,
    ClaimedOutputTombstone,
    NonFungible,
    TransactionReceipt,
    ValidatorFeePool,
    Template,
    Utxo,
    ConfidentialOutput,
}

impl SubstateType {
    pub const fn as_prefix_str(&self) -> &'static str {
        match self {
            SubstateType::Component => address_prefixes::COMPONENT,
            SubstateType::Resource => address_prefixes::RESOURCE,
            SubstateType::Vault => address_prefixes::VAULT,
            SubstateType::ClaimedOutputTombstone => address_prefixes::CLAIMED_OUTPUT_TOMBSTONE,
            SubstateType::NonFungible => address_prefixes::NON_FUNGIBLE,
            SubstateType::TransactionReceipt => address_prefixes::TRANSACTION_RECEIPT,
            SubstateType::ValidatorFeePool => address_prefixes::VALIDATOR_FEE_POOL,
            SubstateType::Template => address_prefixes::TEMPLATE,
            SubstateType::Utxo => address_prefixes::UTXO,
            SubstateType::ConfidentialOutput => address_prefixes::CONFIDENTIAL_OUTPUT,
        }
    }

    pub fn matches(&self, addr: &SubstateId) -> bool {
        #[allow(clippy::match_like_matches_macro)]
        match (self, addr) {
            (SubstateType::Component, SubstateId::Component(_)) => true,
            (SubstateType::Resource, SubstateId::Resource(_)) => true,
            (SubstateType::Vault, SubstateId::Vault(_)) => true,
            (SubstateType::NonFungible, SubstateId::NonFungible(_)) => true,
            (SubstateType::ClaimedOutputTombstone, SubstateId::ClaimedOutputTombstone(_)) => true,
            (SubstateType::TransactionReceipt, SubstateId::TransactionReceipt(_)) => true,
            (SubstateType::ValidatorFeePool, SubstateId::ValidatorFeePool(_)) => true,
            (SubstateType::Template, SubstateId::Template(_)) => true,
            (SubstateType::Utxo, SubstateId::Utxo(_)) => true,
            (SubstateType::ConfidentialOutput, SubstateId::ConfidentialOutput(_)) => true,
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
            SubstateValue::ClaimedOutputTombstone(_) => SubstateType::ClaimedOutputTombstone,
            SubstateValue::NonFungible(_) => SubstateType::NonFungible,
            SubstateValue::TransactionReceipt(_) => SubstateType::TransactionReceipt,
            SubstateValue::Template(_) => SubstateType::Template,
            SubstateValue::ValidatorFeePool(_) => SubstateType::ValidatorFeePool,
            SubstateValue::Utxo(_) => SubstateType::Utxo,
            SubstateValue::ConfidentialOutput(_) => SubstateType::ConfidentialOutput,
        }
    }
}

impl From<&SubstateId> for SubstateType {
    fn from(value: &SubstateId) -> Self {
        match value {
            SubstateId::Component(_) => SubstateType::Component,
            SubstateId::Resource(_) => SubstateType::Resource,
            SubstateId::Vault(_) => SubstateType::Vault,
            SubstateId::ClaimedOutputTombstone(_) => SubstateType::ClaimedOutputTombstone,
            SubstateId::NonFungible(_) => SubstateType::NonFungible,
            SubstateId::TransactionReceipt(_) => SubstateType::TransactionReceipt,
            SubstateId::ValidatorFeePool(_) => SubstateType::ValidatorFeePool,
            SubstateId::Template(_) => SubstateType::Template,
            SubstateId::Utxo(_) => SubstateType::Utxo,
            SubstateId::ConfidentialOutput(_) => SubstateType::ConfidentialOutput,
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
        write!(f, "{:?}", self)
    }
}

/// A substate value paired with an id that addresses a different type of substate.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Substate {id} is a {expected} but its value is a {found}")]
pub struct SubstateTypeMismatch {
    pub id: SubstateId,
    pub expected: SubstateType,
    pub found: SubstateType,
}

/// Checks that `value` is the type of substate that `id` addresses.
pub fn check_substate_type(id: &SubstateId, value: &SubstateValue) -> Result<(), SubstateTypeMismatch> {
    let found = SubstateType::from(value);
    if found.matches(id) {
        return Ok(());
    }
    Err(SubstateTypeMismatch {
        id: id.clone(),
        expected: SubstateType::from(id),
        found,
    })
}

/// Checks that every substate `diff` brings up has a value of the type its id addresses.
pub fn check_diff_substate_types(diff: &SubstateDiff) -> Result<(), SubstateTypeMismatch> {
    diff.up_iter()
        .try_for_each(|(id, substate)| check_substate_type(id, substate.substate_value()))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tari_engine_types::{resource_container::ResourceContainer, vault::Vault};
    use tari_template_lib_types::{ComponentAddress, VaultId, constants::TARI_TOKEN};

    use super::*;

    fn vault_id() -> SubstateId {
        VaultId::from_hex(&"02".repeat(32)).unwrap().into()
    }

    fn vault() -> SubstateValue {
        Vault::new(ResourceContainer::public_fungible(TARI_TOKEN, 1u64)).into()
    }

    #[test]
    fn a_value_of_the_addressed_type_passes() {
        check_substate_type(&vault_id(), &vault()).unwrap();
    }

    #[test]
    fn a_value_of_another_type_is_a_mismatch() {
        let id = SubstateId::from(ComponentAddress::from_array([1; 32]));
        let err = check_substate_type(&id, &vault()).unwrap_err();
        assert!(matches!(err.expected, SubstateType::Component));
        assert!(matches!(err.found, SubstateType::Vault));
    }

    #[test]
    fn a_diff_with_one_mistyped_up_substate_is_a_mismatch() {
        let mut diff = SubstateDiff::new();
        diff.up(vault_id(), Substate::new(1, vault()));
        check_diff_substate_types(&diff).unwrap();

        diff.up(ComponentAddress::from_array([1; 32]).into(), Substate::new(1, vault()));
        check_diff_substate_types(&diff).unwrap_err();
    }

    #[test]
    fn as_prefix_str_matches_substate_id_display_prefix() {
        let hex32 = "7cbfe29101c24924b1b6ccefbfff98986d648622272ae24f7585dab5ffffffff";
        let cases = [
            (SubstateType::Component, format!("component_{hex32}")),
            (SubstateType::Resource, format!("resource_{hex32}")),
            (SubstateType::Vault, format!("vault_{hex32}")),
            (SubstateType::ClaimedOutputTombstone, format!("tombstone_{hex32}")),
            (SubstateType::NonFungible, format!("nft_{hex32}_str_SpecialNft")),
            (SubstateType::TransactionReceipt, format!("txreceipt_{hex32}")),
            (SubstateType::ValidatorFeePool, format!("vnfp_{hex32}")),
            (SubstateType::Template, format!("template_{hex32}")),
            (SubstateType::Utxo, format!("utxo_{hex32}_{hex32}")),
            (SubstateType::ConfidentialOutput, format!("coutput_{hex32}_{hex32}")),
        ];

        for (expected_type, raw_id) in cases {
            let id = SubstateId::from_str(&raw_id).unwrap();
            let substate_type = SubstateType::from(&id);
            assert_eq!(substate_type, expected_type);
            assert!(substate_type.matches(&id));
            let (prefix, _) = id.to_string().split_once('_').unwrap();
            assert_eq!(substate_type.as_prefix_str(), prefix);
        }
    }
}
