use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tempfile::tempdir;

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