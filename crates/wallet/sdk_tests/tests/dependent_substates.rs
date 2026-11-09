//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The wallet declares a component's stored dependencies as transaction inputs, so it must leave out any it holds
//! that no transaction may declare.

mod support;

use std::collections::HashSet;

use tari_engine_types::{
    ValidatorFeePool,
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::{SubstateRequirement, SubstateVersion, VersionedSubstateIdRef};
use tari_template_lib::types::{
    ClaimedOutputTombstoneAddress,
    ComponentAddress,
    Metadata,
    NonFungibleAddress,
    NonFungibleId,
    ObjectKey,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    TransactionReceiptAddress,
    ValidatorFeePoolAddress,
    VaultId,
    access_rules::ResourceAccessRules,
    constants::TARI_TOKEN,
    crypto::RistrettoPublicKeyBytes,
};

use crate::support::{CannedSubstateNetworkInterface, Test, TestWithNetwork};

fn create_test_resource_value(resource_type: ResourceType) -> SubstateValue {
    SubstateValue::Resource(Box::new(Resource::new(
        resource_type,
        SubstateOwnerRule::None,
        ResourceAccessRules::new(),
        Metadata::new(),
        None,
        None,
        0,
        false,
    )))
}

fn create_test_vault_value(resource_address: ResourceAddress, amount: u64) -> SubstateValue {
    Vault::new(ResourceContainer::public_fungible(resource_address, amount)).into()
}

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

#[test]
fn load_dependent_substates_includes_unstored_references_nft_resources_and_handles_cycles() {
    let test = Test::new();
    let substates = test.sdk().substate_api();

    let component_a = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([10; 32])));
    let component_b = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([11; 32])));
    let child_vault = SubstateId::from(VaultId::new(ObjectKey::from_array([12; 32])));
    let unstored_vault_resource = SubstateId::from(ResourceAddress::new(ObjectKey::from_array([13; 32])));
    let nft_resource = ResourceAddress::new(ObjectKey::from_array([14; 32]));
    let unstored_nft = SubstateId::from(NonFungibleAddress::new(nft_resource, NonFungibleId::from_u64(1)));

    // Cyclic reference between stored roots A <-> B, plus an unstored NFT on root A and an unstored resource on child.
    substates
        .save_root(
            VersionedSubstateIdRef::new(&component_a, SubstateVersion::new(1)),
            vec![component_b.clone(), unstored_nft.clone()],
        )
        .unwrap();
    substates
        .save_root(
            VersionedSubstateIdRef::new(&component_b, SubstateVersion::new(2)),
            vec![component_a.clone()],
        )
        .unwrap();
    substates
        .save_child(
            &component_a,
            VersionedSubstateIdRef::new(&child_vault, SubstateVersion::new(3)),
            vec![unstored_vault_resource.clone(), component_a.clone()],
        )
        .unwrap();

    let dependencies = substates.load_dependent_substates(&[&component_a]).unwrap();
    let by_id = dependencies
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::< std::collections::HashMap<_, _>>();

    assert_eq!(by_id.get(&component_a), Some(&Some(SubstateVersion::new(1))));
    assert_eq!(by_id.get(&component_b), Some(&Some(SubstateVersion::new(2))));
    assert_eq!(by_id.get(&child_vault), Some(&Some(SubstateVersion::new(3))));
    assert_eq!(by_id.get(&unstored_vault_resource), Some(&None));
    assert_eq!(by_id.get(&unstored_nft), Some(&None));
    assert_eq!(by_id.get(&SubstateId::Resource(nft_resource)), Some(&None));
    assert_eq!(by_id.len(), 6);
}

#[tokio::test]
async fn locate_dependent_substates_resolves_validator_fee_pool_and_tari_token() {
    let fee_pool_id = SubstateId::from(ValidatorFeePoolAddress::from_array([21; 32]));
    let vault_id = SubstateId::from(VaultId::new(ObjectKey::from_array([22; 32])));
    let tari_res_id = SubstateId::Resource(TARI_TOKEN);

    let network = CannedSubstateNetworkInterface::new([
        (
            fee_pool_id.clone(),
            SubstateVersion::new(4),
            SubstateValue::ValidatorFeePool(ValidatorFeePool::new(RistrettoPublicKeyBytes::default(), 500)),
        ),
        (
            vault_id.clone(),
            SubstateVersion::new(2),
            create_test_vault_value(TARI_TOKEN, 1_000),
        ),
        (
            tari_res_id.clone(),
            SubstateVersion::ZERO,
            create_test_resource_value(ResourceType::Stealth),
        ),
    ]);
    let test = TestWithNetwork::without_accounts(network);
    let substates = test.sdk().substate_api();

    // Case 1: ValidatorFeePool queried on its own must include both ValidatorFeePool and TARI_TOKEN.
    let single = substates
        .locate_dependent_substates(std::slice::from_ref(&fee_pool_id), false)
        .await
        .unwrap();
    let single_map = single
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(single_map.get(&fee_pool_id), Some(&Some(SubstateVersion::new(4))));
    assert_eq!(single_map.get(&tari_res_id), Some(&Some(SubstateVersion::ZERO)));
    assert_eq!(single_map.len(), 2);

    // Case 2: ValidatorFeePool queried after a TARI vault (when TARI_TOKEN is already in `substate_ids`)
    // must still retain the ValidatorFeePool itself.
    let combined = substates
        .locate_dependent_substates(&[vault_id.clone(), fee_pool_id.clone()], true)
        .await
        .unwrap();
    assert_eq!(
        combined,
        HashSet::from([
            SubstateRequirement::unversioned(vault_id),
            SubstateRequirement::unversioned(fee_pool_id),
            SubstateRequirement::unversioned(tari_res_id),
        ])
    );
}

#[tokio::test]
async fn locate_dependent_substates_resolves_versions_for_local_references_and_skips_non_declarable() {
    let vault_id = SubstateId::from(VaultId::new(ObjectKey::from_array([31; 32])));
    let resx_addr = ResourceAddress::new(ObjectKey::from_array([32; 32]));
    let resx_id = SubstateId::Resource(resx_addr);
    let receipt_id = SubstateId::from(TransactionReceiptAddress::from_array([33; 32]));
    let tombstone_id = SubstateId::from(ClaimedOutputTombstoneAddress::new(ObjectKey::from_array([34; 32])));
    let identity_badge = SubstateId::from(NonFungibleAddress::from_public_key(RistrettoPublicKeyBytes::default()));
    let missing_ref_id = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([35; 32])));

    let network = CannedSubstateNetworkInterface::new([(
        resx_id.clone(),
        SubstateVersion::new(7),
        create_test_resource_value(ResourceType::Confidential),
    )]);
    let test = TestWithNetwork::without_accounts(network);
    let substates = test.sdk().substate_api();

    substates
        .save_root(
            VersionedSubstateIdRef::new(&vault_id, SubstateVersion::new(5)),
            vec![resx_id.clone(), receipt_id.clone(), missing_ref_id],
        )
        .unwrap();

    // Non-declarable top-level inputs (receipt, tombstone, virtual identity badge) must be skipped without
    // querying the network, the unstored resource referenced by `vault_id` must have its version fetched
    // when `unversioned = false`, and a stale/down referenced substate not found on the network must be
    // omitted rather than failing the lookup.
    let resolved = substates
        .locate_dependent_substates(&[vault_id.clone(), receipt_id, tombstone_id, identity_badge], false)
        .await
        .unwrap();
    let by_id = resolved
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(by_id.get(&vault_id), Some(&Some(SubstateVersion::new(5))));
    assert_eq!(by_id.get(&resx_id), Some(&Some(SubstateVersion::new(7))));
    assert_eq!(by_id.len(), 2);
}
