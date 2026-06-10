//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A standalone read view (`create_read_tx`) is a consistent point-in-time snapshot: it observes a
//! single committed state for its whole lifetime and is unaffected by commits made after it was
//! opened, whether those commits happen on the same thread or a concurrent one. A *fresh* read view
//! observes the latest committed state. See `crates/state_store_rocksdb/CONTEXT.md` (read view).

pub mod helpers;

use helpers::{create_rocksdb, num_preshards};
use tari_consensus_types::{BlockId, LeafBlock};
use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};

// Compile-time guard: a read view is `Sync` so `&view` can be shared across threads for parallel
// reads on one consistent snapshot. (It stays `!Send` by construction; see `reader.rs`.)
fn _read_view_is_sync<T: Sync>() {}
const _: fn() = || _read_view_is_sync::<tari_state_store_rocksdb::ReadView<'static, String>>();

fn leaf_block_at(epoch: Epoch, height: u64) -> LeafBlock {
    LeafBlock {
        block_id: BlockId::zero(),
        height: NodeHeight(height),
        epoch,
        shard_group: ShardGroup::all_shards(num_preshards()),
    }
}

#[test]
fn read_view_is_isolated_from_a_later_commit() {
    let (db, _tmp) = create_rocksdb();
    let epoch = Epoch::zero();

    // Commit the baseline value.
    db.with_write_tx(|tx| tx.leaf_block_set(&leaf_block_at(epoch, 100)))
        .unwrap();

    // Open a consistent read view over the committed state.
    let view = db.create_read_tx().unwrap();
    assert_eq!(view.leaf_block_get(epoch).unwrap().height, NodeHeight(100));

    // Commit a newer value WHILE the view is still open.
    db.with_write_tx(|tx| tx.leaf_block_set(&leaf_block_at(epoch, 200)))
        .unwrap();

    // The view is a point-in-time snapshot: it must NOT observe the commit made after it was opened.
    // (Against the previous non-snapshot read transaction this read would return 200.)
    assert_eq!(
        view.leaf_block_get(epoch).unwrap().height,
        NodeHeight(100),
        "read view observed a commit made after it was opened"
    );

    // A fresh read view observes the latest committed state.
    let fresh = db.create_read_tx().unwrap();
    assert_eq!(fresh.leaf_block_get(epoch).unwrap().height, NodeHeight(200));
}

#[test]
fn read_view_is_isolated_from_a_concurrent_writer() {
    let (db, _tmp) = create_rocksdb();
    let epoch = Epoch::zero();

    db.with_write_tx(|tx| tx.leaf_block_set(&leaf_block_at(epoch, 100)))
        .unwrap();

    let view = db.create_read_tx().unwrap();
    assert_eq!(view.leaf_block_get(epoch).unwrap().height, NodeHeight(100));

    // A writer on another thread commits a newer value while we hold the view. The view never
    // crosses the thread boundary (only a clone of the store does).
    let writer_db = db.clone();
    std::thread::spawn(move || {
        writer_db
            .with_write_tx(|tx| tx.leaf_block_set(&leaf_block_at(epoch, 200)))
            .unwrap();
    })
    .join()
    .unwrap();

    // The snapshot the view was opened against is unaffected by the concurrent commit.
    assert_eq!(
        view.leaf_block_get(epoch).unwrap().height,
        NodeHeight(100),
        "read view observed a concurrent commit"
    );
    // A fresh view sees it.
    assert_eq!(
        db.create_read_tx().unwrap().leaf_block_get(epoch).unwrap().height,
        NodeHeight(200)
    );
}
