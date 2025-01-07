//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::{rngs::OsRng, RngCore};
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Block, Command, Decision, TransactionAtom, TransactionPoolStage, TransactionPoolStatusUpdate},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_utilities::epoch_time::EpochTime;


mod substates {
    use tari_dan_common_types::{shard::Shard, ExtraData, NumPreshards, ShardGroup};
    use tari_dan_storage::consensus_models::{BlockId, QcId, SubstateDestroyed, SubstateRecord};
    use tari_engine_types::{component::{ComponentBody, ComponentHeader}, substate::{SubstateId, SubstateValue}, TemplateAddress};
    use tari_state_tree::Node;
    use tari_template_lib::{auth::OwnerRule, models::{ComponentAddress, EntityId}, prelude::AccessRules};
    use tari_template_lib::prelude::ComponentAccessRules;
    use tari_transaction::TransactionId;

    use crate::helper::{assert_eq_debug, create_rocksdb, create_sqlite, create_tx_atom};
    use tari_engine_types::serde_with::hex::option;
    

    use super::*;

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

        let substate_id = SubstateId::Component(ComponentAddress::from_hex("0000000000000000000000000000000000000000000000000000000000000000").unwrap());
        let entity_id = substate_id.to_object_key().as_entity_id();
        let substate = SubstateRecord {
            substate_id, 
            version: 0,
            substate_value: SubstateValue::Component(ComponentHeader {
                template_address: TemplateAddress::default(),
                module_name: "foo".to_string(),
                owner_key: None,
                owner_rule: OwnerRule::None,
                access_rules: ComponentAccessRules::allow_all(),
                entity_id,
                body: ComponentBody {
                    state: tari_bor::Value::Null,
                },
            }),
            state_hash: FixedHash::default(),
            created_by_transaction: TransactionId::default(),
            created_justify: QcId::zero(),
            created_block: BlockId::genesis(),
            created_height: NodeHeight::zero(),
            created_by_shard: Shard::zero(),
            created_at_epoch: Epoch::zero(),
            destroyed: None,
        };

        let substate_address = substate.to_substate_address();
        tx.substates_create(&substate).unwrap();

       
        let res = tx.substates_get(&substate_address).unwrap();
        assert_eq_debug(&res, &substate);
     
        tx.rollback().unwrap();
    }

}
