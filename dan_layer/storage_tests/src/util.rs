use tari_dan_storage::consensus_models::{Decision, TransactionAtom};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tempfile::tempdir;

use rand::{rngs::OsRng, RngCore};
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