use tari_common_types::types::FixedHash;
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight};
use tari_dan_storage::consensus_models::{BlockId, Decision, QcId, SubstateRecord, TransactionAtom};
use tari_engine_types::{component::{ComponentBody, ComponentHeader}, substate::{SubstateId, SubstateValue}};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_template_lib::{auth::OwnerRule, models::{ComponentAddress, ComponentKey, EntityId, ObjectKey, TemplateAddress}, prelude::AccessRules};
use tari_template_lib::prelude::ComponentAccessRules;
use tempfile::tempdir;

use rand::{rngs::OsRng, Rng, RngCore};
use tari_transaction::TransactionId;

pub fn create_rocksdb() -> RocksDbStateStore<String> {
    let temp_dir = tempdir().unwrap();
    let db_file = temp_dir.path().join("rocksdb");
    let db_file = db_file
        .as_os_str()
        .to_str().unwrap();

    RocksDbStateStore::connect(db_file).unwrap()
}

pub fn create_sqlite() -> SqliteStateStore<String> {
    let db = SqliteStateStore::connect(":memory:").unwrap();

    // Need FK=off because otherwise we'd have to create transactions for each in the pool
    db.foreign_keys_off().unwrap();
    db
}

pub fn create_tx_atom() -> TransactionAtom {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    TransactionAtom {
        id: TransactionId::new(bytes),
        decision: Decision::Commit,
        evidence: Default::default(),
        transaction_fee: 0,
        leader_fee: None,
    }
}

pub fn create_random_substate_id() -> SubstateId {
    let entity_id = EntityId::default();
    let rand_bytes = OsRng.gen::<[u8; ComponentKey::LENGTH]>();
    let component_key = ComponentKey::new(copy_fixed(&rand_bytes));
    SubstateId::Component(ComponentAddress::new(ObjectKey::new(entity_id, component_key)))
}

pub fn build_substate_record(substate_id: &SubstateId, version: u32) -> SubstateRecord {
    let entity_id = substate_id.to_object_key().as_entity_id();
    SubstateRecord {
            substate_id: substate_id.clone(), 
            version,
            substate_value: build_substate_value(Some(entity_id)),
            state_hash: FixedHash::default(),
            created_by_transaction: TransactionId::default(),
            created_justify: QcId::zero(),
            created_block: BlockId::genesis(),
            created_height: NodeHeight::zero(),
            created_by_shard: Shard::zero(),
            created_at_epoch: Epoch::zero(),
            destroyed: None,
    }
}

pub fn build_substate_value(entity_id: Option<EntityId>) -> SubstateValue {
    SubstateValue::Component(ComponentHeader {
        template_address: TemplateAddress::default(),
        module_name: "foo".to_string(),
        owner_key: None,
        owner_rule: OwnerRule::None,
        access_rules: ComponentAccessRules::allow_all(),
        entity_id: entity_id.unwrap_or_default(),
        body: ComponentBody {
            state: tari_bor::Value::Null,
        },
    })
}

pub fn copy_fixed<const SZ: usize>(bytes: &[u8]) -> [u8; SZ] {
    let mut out = [0u8; SZ];
    out.copy_from_slice(bytes);
    out
}

pub fn assert_eq_debug<T>(a: &T, b: &T)
    where T: std::fmt::Debug
{
    assert_eq!(
        format!("{:?}", a),
        format!("{:?}", b),
    );
}

pub fn create_random_block_id() -> BlockId {
    let rand_bytes = OsRng.gen::<[u8; FixedHash::byte_size()]>();
    BlockId::new(FixedHash::new(rand_bytes))
}
