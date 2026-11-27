//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The wallet declares a component's stored dependencies as transaction inputs, so it must leave out any it holds
//! that no transaction may declare.

mod support;

use std::collections::HashSet;

use tari_engine_types::{
    Utxo,
    UtxoOutput,
    ValidatorFeePool,
    component::{Component, ComponentBody, ComponentHeader},
    confidential_output::ConfidentialOutput,
    crypto::OutputBody,
    non_fungible::NonFungibleContainer,
    resource::Resource,
    resource_container::ResourceContainer,
    substate::{SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::{SubstateRequirement, SubstateVersion, VersionedSubstateIdRef};
use tari_template_lib::types::{
    ClaimedOutputTombstoneAddress,
    ComponentAddress,
    ConfidentialOutputAddress,
    EncryptedData,
    EntityId,
    Metadata,
    NonFungibleAddress,
    NonFungibleId,
    ObjectKey,
    ResourceAddress,
    ResourceType,
    SubstateOwnerRule,
    TemplateAddress,
    TransactionReceiptAddress,
    UtxoAddress,
    UtxoId,
    ValidatorFeePoolAddress,
    VaultId,
    access_rules::{ComponentAccessRules, ResourceAccessRules, RulesUpdater, UpdateRule},
    constants::TARI_TOKEN,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
    stealth::SpendAuthorization,
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
        .collect::<std::collections::HashMap<_, _>>();

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
        .save_root(VersionedSubstateIdRef::new(&vault_id, SubstateVersion::new(5)), vec![
            resx_id.clone(),
            receipt_id.clone(),
            missing_ref_id,
        ])
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

fn create_test_component_value(state: tari_bor::Value) -> SubstateValue {
    SubstateValue::Component(Component {
        header: ComponentHeader {
            template_address: TemplateAddress::from_array([9; 32]),
            owner_rule: SubstateOwnerRule::None,
            access_rules: ComponentAccessRules::new(),
            entity_id: EntityId::default(),
            upgrade_rule: UpdateRule::Locked,
            rules_updater: Some(RulesUpdater::TemplateManaged),
        },
        body: ComponentBody::from_cbor_value(state),
    })
}

#[tokio::test]
async fn locate_dependent_substates_refreshes_stale_stored_component_and_vault_versions_without_duplicate_fetches() {
    let component_id = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([40; 32])));
    let vault_a = VaultId::new(ObjectKey::from_array([41; 32]));
    let vault_a_id = SubstateId::Vault(vault_a);
    let vault_b = VaultId::new(ObjectKey::from_array([42; 32]));
    let vault_b_id = SubstateId::Vault(vault_b);
    let resx_a = ResourceAddress::new(ObjectKey::from_array([43; 32]));
    let resx_a_id = SubstateId::Resource(resx_a);
    let resx_b = ResourceAddress::new(ObjectKey::from_array([44; 32]));
    let resx_b_id = SubstateId::Resource(resx_b);
    let nft_resx = ResourceAddress::new(ObjectKey::from_array([45; 32]));
    let nft_resx_id = SubstateId::Resource(nft_resx);
    let nft_id = SubstateId::from(NonFungibleAddress::new(nft_resx, NonFungibleId::from_u64(7)));

    let live_state = tari_bor::to_value(&(vec![vault_a, vault_b], nft_id.clone())).unwrap();
    let network = CannedSubstateNetworkInterface::new([
        (
            component_id.clone(),
            SubstateVersion::new(3),
            create_test_component_value(live_state),
        ),
        (
            vault_a_id.clone(),
            SubstateVersion::new(5),
            create_test_vault_value(resx_a, 800),
        ),
        (
            vault_b_id.clone(),
            SubstateVersion::new(2),
            create_test_vault_value(resx_b, 300),
        ),
        (
            resx_a_id.clone(),
            SubstateVersion::new(2),
            create_test_resource_value(ResourceType::Fungible),
        ),
        (
            resx_b_id.clone(),
            SubstateVersion::ZERO,
            create_test_resource_value(ResourceType::Fungible),
        ),
        (
            nft_id.clone(),
            SubstateVersion::new(4),
            SubstateValue::NonFungible(NonFungibleContainer::no_data()),
        ),
        (
            nft_resx_id.clone(),
            SubstateVersion::new(1),
            create_test_resource_value(ResourceType::NonFungible),
        ),
    ]);
    let network_probe = network.clone();
    let test = TestWithNetwork::without_accounts(network);
    let substates = test.sdk().substate_api();

    // Seed stale local versions (v0 / v1) before the component, its vault, its resource, and its NFT advanced on-chain.
    substates
        .save_root(VersionedSubstateIdRef::new(&component_id, SubstateVersion::ZERO), vec![
            vault_a_id.clone(),
            nft_id.clone(),
        ])
        .unwrap();
    substates
        .save_child(
            &component_id,
            VersionedSubstateIdRef::new(&vault_a_id, SubstateVersion::ZERO),
            vec![resx_a_id.clone()],
        )
        .unwrap();
    substates
        .save_root(VersionedSubstateIdRef::new(&resx_a_id, SubstateVersion::ZERO), vec![])
        .unwrap();
    substates
        .save_child(
            &nft_resx_id,
            VersionedSubstateIdRef::new(&nft_id, SubstateVersion::new(1)),
            vec![],
        )
        .unwrap();

    let resolved = substates
        .locate_dependent_substates(std::slice::from_ref(&component_id), false)
        .await
        .unwrap();
    let by_id = resolved
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(by_id.get(&component_id), Some(&Some(SubstateVersion::new(3))));
    assert_eq!(by_id.get(&vault_a_id), Some(&Some(SubstateVersion::new(5))));
    assert_eq!(by_id.get(&vault_b_id), Some(&Some(SubstateVersion::new(2))));
    assert_eq!(by_id.get(&resx_a_id), Some(&Some(SubstateVersion::new(2))));
    assert_eq!(by_id.get(&resx_b_id), Some(&Some(SubstateVersion::ZERO)));
    assert_eq!(by_id.get(&nft_id), Some(&Some(SubstateVersion::new(4))));
    assert_eq!(by_id.get(&nft_resx_id), Some(&Some(SubstateVersion::new(1))));
    assert_eq!(by_id.len(), 7);
    assert_eq!(network_probe.query_count(&vault_a_id), 1);
    assert_eq!(network_probe.query_count(&vault_b_id), 1);
}

struct ComponentReferencedResourcesFixture {
    network: CannedSubstateNetworkInterface,
    component_id: SubstateId,
    expected_ids: HashSet<SubstateId>,
    nft_resx: ResourceAddress,
    utxo_resx: ResourceAddress,
    confidential_resx: ResourceAddress,
}

fn component_referenced_resources_fixture() -> ComponentReferencedResourcesFixture {
    let component_id = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([50; 32])));
    let nft_resx = ResourceAddress::new(ObjectKey::from_array([51; 32]));
    let nft_id = SubstateId::from(NonFungibleAddress::new(nft_resx, NonFungibleId::from_u64(1)));
    let utxo_resx = ResourceAddress::new(ObjectKey::from_array([52; 32]));
    let utxo_id = SubstateId::from(UtxoAddress::new(utxo_resx, UtxoId::from_array([53; 32])));
    let confidential_resx = ResourceAddress::new(ObjectKey::from_array([54; 32]));
    let commitment = PedersenCommitmentBytes::from_array([55; 32]);
    let confidential_id = SubstateId::from(ConfidentialOutputAddress::new(confidential_resx, commitment));
    let fee_pool_id = SubstateId::from(ValidatorFeePoolAddress::from_array([56; 32]));
    let identity_badge = SubstateId::from(NonFungibleAddress::from_public_key(RistrettoPublicKeyBytes::default()));

    let state = tari_bor::to_value(&(
        nft_id.clone(),
        utxo_id.clone(),
        confidential_id.clone(),
        fee_pool_id.clone(),
        identity_badge,
    ))
    .unwrap();
    let dummy_output = OutputBody {
        public_nonce: RistrettoPublicKeyBytes::default(),
        encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
        minimum_value_promise: 0,
        viewable_balance: None,
    };
    let utxo_value = Utxo::new(UtxoOutput {
        output: dummy_output.clone(),
        auth: SpendAuthorization::Key(RistrettoPublicKeyBytes::default()),
        tag: UtxoTag::new(0),
    });

    let network = CannedSubstateNetworkInterface::new([
        (
            component_id.clone(),
            SubstateVersion::new(1),
            create_test_component_value(state),
        ),
        (
            nft_id.clone(),
            SubstateVersion::ZERO,
            SubstateValue::NonFungible(NonFungibleContainer::no_data()),
        ),
        (
            SubstateId::Resource(nft_resx),
            SubstateVersion::new(2),
            create_test_resource_value(ResourceType::NonFungible),
        ),
        (utxo_id.clone(), SubstateVersion::ZERO, SubstateValue::Utxo(utxo_value)),
        (
            SubstateId::Resource(utxo_resx),
            SubstateVersion::new(1),
            create_test_resource_value(ResourceType::Stealth),
        ),
        (
            confidential_id.clone(),
            SubstateVersion::ZERO,
            SubstateValue::ConfidentialOutput(ConfidentialOutput::new(dummy_output)),
        ),
        (
            SubstateId::Resource(confidential_resx),
            SubstateVersion::new(3),
            create_test_resource_value(ResourceType::Confidential),
        ),
        (
            fee_pool_id.clone(),
            SubstateVersion::new(6),
            SubstateValue::ValidatorFeePool(ValidatorFeePool::new(RistrettoPublicKeyBytes::default(), 250)),
        ),
        (
            SubstateId::Resource(TARI_TOKEN),
            SubstateVersion::ZERO,
            create_test_resource_value(ResourceType::Stealth),
        ),
    ]);
    let expected_ids = HashSet::from([
        component_id.clone(),
        nft_id,
        SubstateId::Resource(nft_resx),
        utxo_id,
        SubstateId::Resource(utxo_resx),
        confidential_id,
        SubstateId::Resource(confidential_resx),
        fee_pool_id,
        SubstateId::Resource(TARI_TOKEN),
    ]);
    ComponentReferencedResourcesFixture {
        network,
        component_id,
        expected_ids,
        nft_resx,
        utxo_resx,
        confidential_resx,
    }
}

#[tokio::test]
async fn locate_dependent_substates_resolves_associated_resources_for_component_state_references() {
    let f = component_referenced_resources_fixture();
    let test = TestWithNetwork::without_accounts(f.network);
    let substates = test.sdk().substate_api();

    let unversioned = substates
        .locate_dependent_substates(std::slice::from_ref(&f.component_id), true)
        .await
        .unwrap();
    assert_eq!(
        unversioned
            .into_iter()
            .map(SubstateRequirement::into_substate_id)
            .collect::<HashSet<_>>(),
        f.expected_ids
    );

    let versioned = substates
        .locate_dependent_substates(std::slice::from_ref(&f.component_id), false)
        .await
        .unwrap();
    let by_id = versioned
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_id.get(&SubstateId::Resource(f.nft_resx)),
        Some(&Some(SubstateVersion::new(2)))
    );
    assert_eq!(
        by_id.get(&SubstateId::Resource(f.utxo_resx)),
        Some(&Some(SubstateVersion::new(1)))
    );
    assert_eq!(
        by_id.get(&SubstateId::Resource(f.confidential_resx)),
        Some(&Some(SubstateVersion::new(3)))
    );
    assert_eq!(
        by_id.get(&SubstateId::Resource(TARI_TOKEN)),
        Some(&Some(SubstateVersion::ZERO))
    );
    assert_eq!(by_id.len(), f.expected_ids.len());
}

struct PrunedComponentFixture {
    test: TestWithNetwork<CannedSubstateNetworkInterface>,
    component_id: SubstateId,
    vault_a_id: SubstateId,
    resx_a_id: SubstateId,
    nft_dropped_id: SubstateId,
    nft_dropped_resx_id: SubstateId,
    nft_live_id: SubstateId,
    nft_live_resx_id: SubstateId,
    transitive_resx_id: SubstateId,
}

fn pruned_component_references_fixture() -> PrunedComponentFixture {
    let component_id = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([60; 32])));
    let vault_a = VaultId::new(ObjectKey::from_array([61; 32]));
    let vault_a_id = SubstateId::Vault(vault_a);
    let resx_a = ResourceAddress::new(ObjectKey::from_array([62; 32]));
    let resx_a_id = SubstateId::Resource(resx_a);
    let nft_dropped_resx = ResourceAddress::new(ObjectKey::from_array([63; 32]));
    let nft_dropped_resx_id = SubstateId::Resource(nft_dropped_resx);
    let nft_dropped_id = SubstateId::from(NonFungibleAddress::new(nft_dropped_resx, NonFungibleId::from_u64(1)));
    let nft_live_resx = ResourceAddress::new(ObjectKey::from_array([64; 32]));
    let nft_live_resx_id = SubstateId::Resource(nft_live_resx);
    let nft_live_id = SubstateId::from(NonFungibleAddress::new(nft_live_resx, NonFungibleId::from_u64(2)));
    let transitive_resx = ResourceAddress::new(ObjectKey::from_array([65; 32]));
    let transitive_resx_id = SubstateId::Resource(transitive_resx);

    let live_state = tari_bor::to_value(&(vec![vault_a], nft_live_id.clone())).unwrap();
    let network = CannedSubstateNetworkInterface::new([
        (
            component_id.clone(),
            SubstateVersion::new(2),
            create_test_component_value(live_state),
        ),
        (
            vault_a_id.clone(),
            SubstateVersion::new(3),
            create_test_vault_value(resx_a, 500),
        ),
        (
            resx_a_id.clone(),
            SubstateVersion::new(1),
            create_test_resource_value(ResourceType::Fungible),
        ),
        (
            nft_live_id.clone(),
            SubstateVersion::new(2),
            SubstateValue::NonFungible(NonFungibleContainer::no_data()),
        ),
        (
            nft_live_resx_id.clone(),
            SubstateVersion::new(1),
            create_test_resource_value(ResourceType::NonFungible),
        ),
        (
            transitive_resx_id.clone(),
            SubstateVersion::new(4),
            create_test_resource_value(ResourceType::Fungible),
        ),
    ]);
    let test = TestWithNetwork::without_accounts(network);
    let substates = test.sdk().substate_api();

    substates
        .save_root(VersionedSubstateIdRef::new(&component_id, SubstateVersion::ZERO), vec![
            vault_a_id.clone(),
            nft_dropped_id.clone(),
            nft_live_id.clone(),
        ])
        .unwrap();
    substates
        .save_child(
            &nft_dropped_resx_id,
            VersionedSubstateIdRef::new(&nft_dropped_id, SubstateVersion::ZERO),
            vec![],
        )
        .unwrap();
    substates
        .save_child(
            &nft_live_resx_id,
            VersionedSubstateIdRef::new(&nft_live_id, SubstateVersion::ZERO),
            vec![transitive_resx_id.clone()],
        )
        .unwrap();
    substates
        .save_root(
            VersionedSubstateIdRef::new(&transitive_resx_id, SubstateVersion::ZERO),
            vec![],
        )
        .unwrap();

    PrunedComponentFixture {
        test,
        component_id,
        vault_a_id,
        resx_a_id,
        nft_dropped_id,
        nft_dropped_resx_id,
        nft_live_id,
        nft_live_resx_id,
        transitive_resx_id,
    }
}

#[tokio::test]
async fn locate_dependent_substates_drops_pruned_local_component_references_and_keeps_live_transitive_links() {
    let f = pruned_component_references_fixture();
    let substates = f.test.sdk().substate_api();

    let unversioned = substates
        .locate_dependent_substates(std::slice::from_ref(&f.component_id), true)
        .await
        .unwrap();
    let unversioned_ids = unversioned
        .into_iter()
        .map(SubstateRequirement::into_substate_id)
        .collect::<HashSet<_>>();
    assert_eq!(
        unversioned_ids,
        HashSet::from([
            f.component_id.clone(),
            f.vault_a_id.clone(),
            f.resx_a_id.clone(),
            f.nft_live_id.clone(),
            f.nft_live_resx_id.clone(),
            f.transitive_resx_id.clone(),
        ])
    );

    let versioned = substates
        .locate_dependent_substates(std::slice::from_ref(&f.component_id), false)
        .await
        .unwrap();
    let by_id = versioned
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::<std::collections::HashMap<_, _>>();
    assert!(!by_id.contains_key(&f.nft_dropped_id));
    assert!(!by_id.contains_key(&f.nft_dropped_resx_id));
    assert_eq!(by_id.get(&f.component_id), Some(&Some(SubstateVersion::new(2))));
    assert_eq!(by_id.get(&f.vault_a_id), Some(&Some(SubstateVersion::new(3))));
    assert_eq!(by_id.get(&f.nft_live_id), Some(&Some(SubstateVersion::new(2))));
    assert_eq!(by_id.get(&f.transitive_resx_id), Some(&Some(SubstateVersion::new(4))));
    assert_eq!(by_id.len(), 6);
}

#[tokio::test]
async fn locate_dependent_substates_omits_spent_substates_referenced_in_component_state() {
    let component_id = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([70; 32])));
    let live_utxo_resx = ResourceAddress::new(ObjectKey::from_array([71; 32]));
    let live_utxo_id = SubstateId::from(UtxoAddress::new(live_utxo_resx, UtxoId::from_array([72; 32])));
    let spent_utxo_resx = ResourceAddress::new(ObjectKey::from_array([73; 32]));
    let spent_utxo_id = SubstateId::from(UtxoAddress::new(spent_utxo_resx, UtxoId::from_array([74; 32])));
    let spent_confidential_resx = ResourceAddress::new(ObjectKey::from_array([75; 32]));
    let spent_confidential_id = SubstateId::from(ConfidentialOutputAddress::new(
        spent_confidential_resx,
        PedersenCommitmentBytes::from_array([76; 32]),
    ));

    let state = tari_bor::to_value(&(
        live_utxo_id.clone(),
        spent_utxo_id.clone(),
        spent_utxo_id.clone(),
        spent_confidential_id.clone(),
    ))
    .unwrap();
    let dummy_output = OutputBody {
        public_nonce: RistrettoPublicKeyBytes::default(),
        encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
        minimum_value_promise: 0,
        viewable_balance: None,
    };
    let utxo_value = Utxo::new(UtxoOutput {
        output: dummy_output,
        auth: SpendAuthorization::Key(RistrettoPublicKeyBytes::default()),
        tag: UtxoTag::new(0),
    });

    let network = CannedSubstateNetworkInterface::new([
        (
            component_id.clone(),
            SubstateVersion::new(1),
            create_test_component_value(state),
        ),
        (
            live_utxo_id.clone(),
            SubstateVersion::ZERO,
            SubstateValue::Utxo(utxo_value),
        ),
        (
            SubstateId::Resource(live_utxo_resx),
            SubstateVersion::new(2),
            create_test_resource_value(ResourceType::Stealth),
        ),
        (
            SubstateId::Resource(spent_utxo_resx),
            SubstateVersion::new(1),
            create_test_resource_value(ResourceType::Stealth),
        ),
        (
            SubstateId::Resource(spent_confidential_resx),
            SubstateVersion::new(1),
            create_test_resource_value(ResourceType::Confidential),
        ),
    ]);
    let network_probe = network.clone();
    let test = TestWithNetwork::without_accounts(network);
    let substates = test.sdk().substate_api();

    let versioned = substates
        .locate_dependent_substates(std::slice::from_ref(&component_id), false)
        .await
        .unwrap();
    let by_id = versioned
        .iter()
        .map(|req| (req.substate_id().clone(), req.version()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(by_id.get(&component_id), Some(&Some(SubstateVersion::new(1))));
    assert_eq!(by_id.get(&live_utxo_id), Some(&Some(SubstateVersion::ZERO)));
    assert_eq!(
        by_id.get(&SubstateId::Resource(live_utxo_resx)),
        Some(&Some(SubstateVersion::new(2)))
    );
    assert!(!by_id.contains_key(&spent_utxo_id));
    assert!(!by_id.contains_key(&SubstateId::Resource(spent_utxo_resx)));
    assert!(!by_id.contains_key(&spent_confidential_id));
    assert!(!by_id.contains_key(&SubstateId::Resource(spent_confidential_resx)));
    assert_eq!(by_id.len(), 3);
    assert_eq!(network_probe.query_count(&spent_utxo_id), 1);
}
