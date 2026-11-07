//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The wallet declares a component's stored dependencies as transaction inputs, so it must leave out any it holds
//! that no transaction may declare.

mod support;

use std::collections::HashSet;

use tari_engine_types::{
    ValidatorFeePool,
    confidential::ClaimedOutputTombstone,
    substate::{SubstateId, SubstateValue},
};
use tari_ootle_common_types::{SubstateRequirement, SubstateVersion, VersionedSubstateIdRef};
use tari_ootle_light_client::test_fixtures::ValidatorSetFixture;
use tari_template_lib::types::{
    ClaimedOutputTombstoneAddress,
    ComponentAddress,
    ObjectKey,
    TransactionReceiptAddress,
    ValidatorFeePoolAddress,
    VaultId,
    constants::TARI_TOKEN,
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

#[tokio::test]
async fn locate_dependent_substates_leaves_out_output_only_substates_and_adds_fee_pool_resource() {
    let sets = [ValidatorSetFixture::new(1), ValidatorSetFixture::new(2)];
    let network = ProvingNetwork::new(&sets, 0);
    let test = TestWithNetwork::with_network(network.clone());
    let substates = test.sdk().substate_api();

    let receipt = SubstateId::TransactionReceipt(TransactionReceiptAddress::from_array([5; 32]));
    substates
        .save_root(VersionedSubstateIdRef::new(&receipt, SubstateVersion::ZERO), vec![])
        .unwrap();

    let tombstone =
        SubstateId::ClaimedOutputTombstone(ClaimedOutputTombstoneAddress::new(ObjectKey::from_array([6; 32])));
    network.serve_substate(
        tombstone.clone(),
        SubstateVersion::ZERO,
        SubstateValue::ClaimedOutputTombstone(ClaimedOutputTombstone { value: 42 }),
        None,
    );

    let fee_pool = SubstateId::ValidatorFeePool(ValidatorFeePoolAddress::from_array([7; 32]));
    network.serve_substate(
        fee_pool.clone(),
        SubstateVersion::new(2),
        SubstateValue::ValidatorFeePool(ValidatorFeePool::new(Default::default(), 100)),
        None,
    );

    let dependencies = substates
        .locate_dependent_substates(&[receipt, tombstone, fee_pool.clone()], true)
        .await
        .unwrap();

    assert_eq!(
        dependencies,
        HashSet::from([
            SubstateRequirement::unversioned(fee_pool),
            SubstateRequirement::unversioned(SubstateId::Resource(TARI_TOKEN)),
        ])
    );
}
