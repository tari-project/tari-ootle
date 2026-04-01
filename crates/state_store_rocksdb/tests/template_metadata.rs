//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use helpers::{create_rocksdb, create_substate_update_batch, num_preshards};
use tari_engine_types::{
    published_template::{PublishedTemplate, PublishedTemplateAddress, TemplateMetadata},
    substate::{SubstateId, SubstateValue, hash_substate},
};
use tari_ootle_common_types::{Epoch, Network, VersionedSubstateIdRef};
use tari_ootle_storage::{
    StateStore,
    StateStoreWriteTransaction,
    consensus_models::{Block, SubstateCreated, SubstateRecord},
};
use tari_state_tree::Version;
use tari_template_lib_types::{Hash32, TemplateAddress, crypto::RistrettoPublicKeyBytes};

fn build_template_substate_record(template_address: TemplateAddress, state_version: Version) -> SubstateRecord {
    let published = PublishedTemplate {
        author: RistrettoPublicKeyBytes::default(),
        binary: Default::default(),
        at_epoch: 1,
    };
    let published_addr = PublishedTemplateAddress::from_hash(template_address);
    let substate_id = SubstateId::Template(published_addr);
    let value = SubstateValue::Template(published);
    SubstateRecord {
        substate_id: substate_id.clone(),
        version: 0,
        state_hash: hash_substate(&value, 0),
        substate_value: Some(value),
        created: SubstateCreated {
            at_epoch: Epoch::zero(),
            in_shard: VersionedSubstateIdRef::new(&substate_id, 0).to_shard(num_preshards()),
            at_state_version: state_version,
        },
        destroyed: None,
    }
}

/// Verifies that `scan_template_addresses_missing_metadata` correctly identifies template substates
/// that lack a metadata entry, and that writing the metadata entry causes them to no longer appear.
///
/// This is the only test for the backfill code path: templates published before the metadata worker
/// was deployed are handled by `backfill_missing` which calls `scan_template_addresses_missing_metadata`.
#[test]
fn scan_finds_templates_missing_metadata_and_not_those_with_it() {
    let (db, _tmp) = create_rocksdb();

    let template_address_1 = TemplateAddress::from([1u8; 32]);
    let template_address_2 = TemplateAddress::from([2u8; 32]);

    // Write two template substates to the store.
    let mut tx = db.create_write_tx().unwrap();
    Block::zero_block(Network::LocalNet, num_preshards())
        .insert(&mut tx)
        .unwrap();
    let record1 = build_template_substate_record(template_address_1, 1);
    let record2 = build_template_substate_record(template_address_2, 2);
    let batch = create_substate_update_batch(Epoch::zero(), [&record1, &record2]);
    tx.substates_commit_batch(batch).unwrap();
    tx.commit().unwrap();

    // Both templates lack metadata — scan must return both addresses.
    let mut missing = db.scan_template_addresses_missing_metadata().unwrap();
    missing.sort();
    let mut expected = vec![template_address_1, template_address_2];
    expected.sort();
    assert_eq!(missing, expected, "Both templates should be missing metadata");

    // Write metadata for template_1 only.
    let metadata = TemplateMetadata {
        template_name: "test_template".to_string(),
        author_public_key: RistrettoPublicKeyBytes::default(),
        binary_hash: Hash32::default(),
        at_epoch: 1,
    };
    let mut tx = db.create_write_tx().unwrap();
    tx.template_metadata_upsert(&template_address_1, &metadata).unwrap();
    tx.commit().unwrap();

    // Only template_2 should still be missing metadata.
    let missing = db.scan_template_addresses_missing_metadata().unwrap();
    assert_eq!(
        missing,
        vec![template_address_2],
        "Only template_2 should be missing metadata"
    );

    // Write metadata for template_2.
    let mut tx = db.create_write_tx().unwrap();
    tx.template_metadata_upsert(&template_address_2, &metadata).unwrap();
    tx.commit().unwrap();

    // All templates have metadata — scan should return empty.
    let missing = db.scan_template_addresses_missing_metadata().unwrap();
    assert!(
        missing.is_empty(),
        "All templates have metadata — scan should return empty"
    );
}
