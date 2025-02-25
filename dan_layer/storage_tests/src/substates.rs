//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::{rngs::OsRng, RngCore};
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Block, BlockId, Command, Decision, QcId, SubstateRecord, TransactionAtom, TransactionPoolStage, TransactionPoolStatusUpdate},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_engine_types::{component::{ComponentBody, ComponentHeader}, substate::{SubstateId, SubstateValue}};
use tari_template_lib::{auth::OwnerRule, models::{ComponentAddress, ComponentKey, EntityId, ObjectKey, TemplateAddress}, prelude::AccessRules};
use tari_transaction::TransactionId;
use tari_utilities::epoch_time::EpochTime;
use tari_template_lib::prelude::ComponentAccessRules;

fn random_substate_id() -> SubstateId {
    let rng = &mut OsRng;

    let mut bytes = [0u8; EntityId::LENGTH];
    rng.fill_bytes(&mut bytes);
    let entity_id = EntityId::from_array(bytes);

    let mut bytes = [0u8; ComponentKey::LENGTH];
    rng.fill_bytes(&mut bytes);
    let component_key = ComponentKey::new(bytes); 

    let address = ComponentAddress::new(ObjectKey::new(entity_id, component_key));
    SubstateId::Component(address)
}

mod substates {
    use std::collections::HashSet;

    use tari_dan_common_types::{shard::Shard, SubstateRequirement, VersionedSubstateId, VersionedSubstateIdRef};
    use tari_dan_storage::consensus_models::QcId;
    use tari_transaction::TransactionId;

    use crate::helper::{assert_eq_debug, build_substate_record, create_rocksdb, create_sqlite};
    
    use super::*;

    #[ignore]
    #[test]
    fn basic_substate_operations_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        basic_substate_operations(db);
    }

    #[test]
    fn basic_substate_operations_rocksdb() {
        let db = create_rocksdb();
        basic_substate_operations(db);
    }

    fn basic_substate_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // substate 1
        let substate1_id = random_substate_id();
        let substate1 = build_substate_record(&substate1_id, 0);
        let substate1_address = substate1.to_substate_address();
        tx.substates_create(&substate1).unwrap();

        // substate 1 (version 1)
        let substate1b = build_substate_record(&substate1_id, 1);
        let substate1b_address = substate1b.to_substate_address();
        tx.substates_create(&substate1b).unwrap();

        // substate 2
        let substate2_id = random_substate_id();
        let substate2 = build_substate_record(&substate2_id, 0);
        let substate2_address = substate2.to_substate_address();
        tx.substates_create(&substate2).unwrap();

        // check that we can get all the newly inserted substates
        let res = tx.substates_get(&substate1_address).unwrap();
        assert_eq_debug(&res, &substate1);

        let res = tx.substates_get(&substate1b_address).unwrap();
        assert_eq_debug(&res, &substate1b);

        let res = tx.substates_get(&substate2_address).unwrap();
        assert_eq_debug(&res, &substate2);

        // substates_get_any fetches all substates
        let mut req = HashSet::new();
        req.insert(VersionedSubstateIdRef::new(&substate1_id, 0) );
        req.insert(VersionedSubstateIdRef::new(&substate2_id, 0) );
        let res = tx.substates_get_any(&req).unwrap();
        assert_eq!(res.len(), 2);

        // substates_get_any fetches the last version of a substate
        let mut req = HashSet::new();
        req.insert(VersionedSubstateIdRef::new(&substate1_id, 0) );
        let res = tx.substates_get_any(&req).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq_debug(&res[0], &substate1b);

        // substates_get_any_max_version
        let substate_ids = vec![substate1_id.clone(), substate2_id.clone()];
        let res = tx.substates_get_any_max_version(&substate_ids).unwrap();
        assert_eq!(res.len(), 2);
        assert!(res.iter().any(|s| s.substate_id == substate1_id && s.version == 1));
        assert!(res.iter().any(|s| s.substate_id == substate2_id && s.version == 0));

        // substates_get_max_version_for_substate
        let res = tx.substates_get_max_version_for_substate(&substate1_id).unwrap();
        assert_eq!(res, (1, false));
        let res = tx.substates_get_max_version_for_substate(&substate2_id).unwrap();
        assert_eq!(res, (0, false));

        // substates_any_exist (all exist)
        let substate_ids = vec![
            VersionedSubstateId::new(substate1_id.clone(), 0),
            VersionedSubstateId::new(substate2_id.clone(), 0)
        ];
        let res = tx.substates_any_exist(substate_ids).unwrap();
        assert_eq!(res, true);

        // substates_any_exist (some do not exist)
        let substate_ids = vec![
            VersionedSubstateId::new(substate1_id.clone(), 100), // version should not exist
            VersionedSubstateId::new(substate2_id.clone(), 0)
        ];
        let res = tx.substates_any_exist(substate_ids).unwrap();
        assert_eq!(res, true);

        // substates_any_exist (none exist)
        let substate_ids = vec![
            VersionedSubstateId::new(substate1_id, 100), // version should not exist
            VersionedSubstateId::new(substate2_id, 100) // version should not exist
        ];
        let res = tx.substates_any_exist(substate_ids).unwrap();
        assert_eq!(res, false);

        // substates_get_many_by_created_transaction
        let tx_id = TransactionId::default();
        let res = tx.substates_get_many_by_created_transaction(&tx_id).unwrap();
        assert_eq!(res.len(), 3);

        // substates_get_all_for_transaction
        let tx_id = TransactionId::default();
        let res = tx.substates_get_all_for_transaction(&tx_id).unwrap();
        assert_eq!(res.len(), 3);

        // substates_down
        let res = tx.substates_get(&substate2_address).unwrap();
        assert!(res.destroyed.is_none());

        let versioned_substate_id = VersionedSubstateId::new(substate2.substate_id, substate2.version);
        let shard = Shard::first();
        let epoch = Epoch::zero();
        let destroyed_block_height = NodeHeight::zero();
        let destroyed_transaction_id = TransactionId::default();
        let destroyed_qc_id = QcId::zero();

        tx.substates_down(versioned_substate_id, shard, epoch, destroyed_block_height, &destroyed_transaction_id, &destroyed_qc_id).unwrap();
        let res = tx.substates_get(&substate2_address).unwrap();
        assert!(res.destroyed.is_some());

        tx.rollback().unwrap();
    }

}
