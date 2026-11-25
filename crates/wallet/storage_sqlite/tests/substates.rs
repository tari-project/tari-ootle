//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, str::FromStr};

use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{
    SubstateVersion,
    VersionedSubstateIdRef,
    optional::Optional,
    substate_type::SubstateType,
};
use tari_ootle_wallet_sdk::storage::{
    CommittableStore,
    ReadableWalletStore,
    WalletStoreReader,
    WalletStoreWriter,
    WriteableWalletStore,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_lib_types::{NonFungibleAddress, NonFungibleId, ObjectKey, ResourceAddress, TemplateAddress};

#[test]
fn get_and_insert_substates() {
    let example_addr = "component_1f019e4d434cbf2b99c0af89ee212f422af86de7280a169d2e392dfbffffffff"
        .parse()
        .unwrap();

    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();
    let mut tx = db.create_write_tx().unwrap();
    let substate = tx.substates_get(&example_addr).optional().unwrap();
    assert!(substate.is_none());
    let address =
        SubstateId::from_str("component_1f019e4d434cbf2b99c0af89ee212f422af86de7280a169d2e392dfbffffffff").unwrap();
    tx.substates_upsert_root(
        VersionedSubstateIdRef::new(&address, SubstateVersion::ZERO),
        HashSet::new(),
        None,
        None,
    )
    .unwrap();

    let child_address =
        SubstateId::from_str("component_d9e4a7ce7dbaa73ce10aabf309dd702054756a813f454ef13564f298ffffffff").unwrap();
    tx.substates_upsert_child(
        &address,
        VersionedSubstateIdRef::new(&child_address, SubstateVersion::ZERO),
        HashSet::new(),
    )
    .unwrap();

    tx.commit().unwrap();

    let mut tx = db.create_read_tx().unwrap();

    let returned = tx.substates_get(&address).unwrap();
    assert!(returned.parent_address.is_none());
    assert_eq!(*returned.substate_id.substate_id(), address);
    assert_eq!(returned.substate_id.version(), SubstateVersion::ZERO);

    let returned = tx.substates_get(&child_address).unwrap();
    assert_eq!(returned.parent_address, Some(address));
    assert_eq!(*returned.substate_id.substate_id(), child_address);
    assert_eq!(returned.substate_id.version(), SubstateVersion::ZERO);
}

#[test]
fn substates_get_all_returns_non_fungible_substates() {
    let db = SqliteWalletStore::try_open(":memory:").unwrap();
    db.run_migrations().unwrap();

    let resource_address = ResourceAddress::new(ObjectKey::from_array([5u8; ObjectKey::LENGTH]));
    let nft_address = NonFungibleAddress::new(resource_address, NonFungibleId::from_u64(42));
    let resource_id = SubstateId::Resource(resource_address);
    let nft_id = SubstateId::NonFungible(nft_address);

    let mut tx = db.create_write_tx().unwrap();
    tx.substates_upsert_root(
        VersionedSubstateIdRef::new(&resource_id, SubstateVersion::new(0)),
        HashSet::new(),
        None,
        Some(TemplateAddress::default()),
    )
    .unwrap();
    tx.substates_upsert_child(
        &resource_id,
        VersionedSubstateIdRef::new(&nft_id, SubstateVersion::new(1)),
        HashSet::new(),
    )
    .unwrap();

    let nfts = tx
        .substates_get_all(Some(SubstateType::NonFungible), None, None, None)
        .unwrap();
    assert_eq!(nfts.len(), 1);
    assert_eq!(*nfts[0].substate_id.substate_id(), nft_id);
    tx.commit().unwrap();
}
