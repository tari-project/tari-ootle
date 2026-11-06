//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The wallet declares a component's stored dependencies as transaction inputs, so it must leave out any it holds
//! that no transaction may declare.

mod support;

use std::collections::HashSet;

use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateIdRef};
use tari_template_lib::types::{
    ComponentAddress,
    NonFungibleAddress,
    NonFungibleId,
    ObjectKey,
    ResourceAddress,
    TransactionReceiptAddress,
    VaultId,
};

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

#[test]
fn loading_dependent_substates_includes_nft_resources_and_preserves_stored_versions() {
    let test = Test::new();
    let substates = test.sdk().substate_api();

    let nft_resource = ResourceAddress::new(ObjectKey::from_array([6; 32]));
    let nft_resource_id = SubstateId::from(nft_resource);
    let nft = SubstateId::from(NonFungibleAddress::new(nft_resource, NonFungibleId::from_u64(1)));
    let component = SubstateId::from(ComponentAddress::new(ObjectKey::from_array([7; 32])));

    // Store the NFT resource at v3 and the NFT itself at v2, referenced directly by the component.
    substates
        .save_root(
            VersionedSubstateIdRef::new(&nft_resource_id, SubstateVersion::new(3)),
            [],
        )
        .unwrap();
    substates
        .save_child(
            &nft_resource_id,
            VersionedSubstateIdRef::new(&nft, SubstateVersion::new(2)),
            [],
        )
        .unwrap();
    substates
        .save_root(VersionedSubstateIdRef::new(&component, SubstateVersion::new(1)), [
            nft.clone()
        ])
        .unwrap();

    let dependencies = substates.load_dependent_substates(&[&component]).unwrap();
    assert!(
        dependencies.iter().any(|req| req.substate_id() == &nft_resource_id),
        "expected NFT resource to be included in dependencies, got: {dependencies:?}"
    );
    let nft_req = dependencies
        .iter()
        .find(|req| req.substate_id() == &nft)
        .expect("expected NFT to be included in dependencies");
    assert_eq!(nft_req.version(), Some(SubstateVersion::new(2)));
}
