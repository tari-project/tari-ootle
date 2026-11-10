//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The wallet declares a component's stored dependencies as transaction inputs, so it must leave out any it holds
//! that no transaction may declare.

mod support;

use std::collections::HashSet;

use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateIdRef};
use tari_template_lib::types::{ComponentAddress, ObjectKey, TransactionReceiptAddress, VaultId};

use crate::support::Test;

#[test]
fn a_components_dependencies_leave_out_the_receipts_it_references() {
    let test = Test::new();
    let substates = test.sdk().substate_api();

    let receipt = SubstateId::from(TransactionReceiptAddress::from_array([3; 32]));
    let vault = SubstateId::from(VaultId::new(ObjectKey::from_array([4; 32])));
    let component = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([5; 32])));
    for (id, referenced) in [
        (&receipt, vec![]),
        (&vault, vec![]),
        (&component, vec![receipt.clone(), vault.clone()]),
    ] {
        substates
            .save_root(VersionedSubstateIdRef::new(id, SubstateVersion::ZERO), referenced)
            .unwrap();
    }

    let dependencies = substates
        .load_dependent_substates(&[&component])
        .unwrap()
        .into_iter()
        .map(|requirement| requirement.into_substate_id())
        .collect::<HashSet<_>>();
    assert_eq!(dependencies, HashSet::from([component, vault]));
}

#[tokio::test]
async fn locating_validator_fee_pool_dependencies_includes_tari_resource() {
    use tari_engine_types::{ValidatorFeePool, resource::Resource, substate::SubstateValue};
    use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
    use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId};
    use tari_template_lib::types::{
        Metadata,
        ResourceType,
        SubstateOwnerRule,
        ValidatorFeePoolAddress,
        access_rules::{ComponentAccessRules, ResourceAccessRules},
        constants::TARI_TOKEN,
        crypto::RistrettoPublicKeyBytes,
    };

    use crate::support::{ProvingNetwork, TestWithNetwork};

    let sets = (1..=2).map(ValidatorSetFixture::new).collect::<Vec<_>>();
    let network = ProvingNetwork::new(&sets, 0);
    let pool_id = SubstateId::ValidatorFeePool(ValidatorFeePoolAddress::from_array([0x77; 32]));
    network.serve_substate(
        pool_id.clone(),
        SubstateVersion::ZERO,
        SubstateValue::ValidatorFeePool(ValidatorFeePool::new(RistrettoPublicKeyBytes::zero(), 1000)),
        None,
    );
    network.serve_substate(
        TARI_TOKEN.into(),
        SubstateVersion::ZERO,
        Resource::new(
            ResourceType::Stealth,
            SubstateOwnerRule::None,
            ResourceAccessRules::new(),
            Metadata::new(),
            None,
            None,
            6,
            true,
        )
        .into(),
        None,
    );

    let test = TestWithNetwork::without_accounts(network);
    let deps = test
        .sdk()
        .substate_api()
        .locate_dependent_substates(std::slice::from_ref(&pool_id), true)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.into_substate_id())
        .collect::<HashSet<_>>();

    assert!(
        deps.contains(&SubstateId::Resource(TARI_TOKEN)),
        "locating dependencies of a ValidatorFeePool must include the TARI resource: {deps:?}"
    );
    assert!(deps.contains(&pool_id));

    let _ = (
        KeyBranch::Account,
        KeyId::derived(KeyBranch::Account, 0),
        ComponentAccessRules::new(),
    );
}
