use tari_dan_storage::consensus_models::{Decision, TransactionAtom};
use tari_engine_types::substate::SubstateId;
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_template_lib::models::{ComponentAddress, ComponentKey, EntityId, ObjectKey};
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
