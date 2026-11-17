//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The wallet declares a component's stored dependencies as transaction inputs, so it must leave out any it holds
//! that no transaction may declare.

mod support;

use std::collections::HashSet;

use tari_engine_types::{
    ValidatorFeePool,
    resource::Resource,
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateIdRef};
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_template_lib::types::{
    ComponentAddress,
    Metadata,
    ObjectKey,
    ResourceType,
    SubstateOwnerRule,
    TransactionReceiptAddress,
    ValidatorFeePoolAddress,
    VaultId,
    access_rules::ResourceAccessRules,
    constants::TARI_TOKEN,
    crypto::RistrettoPublicKeyBytes,
};

use crate::support::{ProvingNetwork, Test, TestWithNetwork};

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

#[tokio::test(flavor = "multi_thread")]
async fn locate_dependent_substates_preserves_validator_fee_pool_when_tari_token_is_present() {
    let sets = [ValidatorSetFixture::new(1)];
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::without_accounts(network.clone());
    let substates = test.sdk().substate_api();

    let tari_id = SubstateId::Resource(TARI_TOKEN);
    let tari_resource = SubstateValue::Resource(Box::new(Resource::new(
        ResourceType::Stealth,
        SubstateOwnerRule::None,
        ResourceAccessRules::new(),
        Metadata::new(),
        None,
        None,
        6,
        false,
    )));
    let pool_id = SubstateId::ValidatorFeePool(ValidatorFeePoolAddress::from_array([7; 32]));
    let pool_value = SubstateValue::ValidatorFeePool(ValidatorFeePool::new(RistrettoPublicKeyBytes::zero(), 5));

    network.serve_substate(tari_id.clone(), SubstateVersion::ZERO, tari_resource, None);
    network.serve_substate(pool_id.clone(), SubstateVersion::ZERO, pool_value, None);

    let deps = substates
        .locate_dependent_substates(&[tari_id.clone(), pool_id.clone()], true)
        .await
        .unwrap()
        .into_iter()
        .map(|req| req.into_substate_id())
        .collect::<HashSet<_>>();
    assert_eq!(deps, HashSet::from([tari_id.clone(), pool_id.clone()]));

    let reverse_deps = substates
        .locate_dependent_substates(std::slice::from_ref(&pool_id), true)
        .await
        .unwrap()
        .into_iter()
        .map(|req| req.into_substate_id())
        .collect::<HashSet<_>>();
    assert_eq!(reverse_deps, HashSet::from([tari_id, pool_id]));
}
