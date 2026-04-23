//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use helpers::create_rocksdb;
use tari_consensus_types::{BlockId, ConsensusDirective, DirectiveBody, DirectiveKind, DirectiveSignature};
use tari_ootle_common_types::{Epoch, optional::Optional};
use tari_ootle_storage::{StateStore, StorageError, consensus_models::AppliedDirective};

fn sample_directive(nonce: u64) -> ConsensusDirective {
    // Build via from_parts using a zero signature; we only care about the ID + body for the
    // persistence tests — verification is covered in the consensus_types directive suite.
    let body = DirectiveBody {
        kind: DirectiveKind::rollback_to_epoch(Epoch(10)),
        nonce,
        issued_at_unix_secs: 1_700_000_000,
    };
    let signature = DirectiveSignature::new([0u8; 32], [0u8; 32]);
    ConsensusDirective::from_parts(body, signature)
}

fn applied_record(directive: &ConsensusDirective, epoch: u64) -> AppliedDirective {
    AppliedDirective {
        directive_id: directive.id(),
        body: directive.body().clone(),
        applied_at_epoch: Epoch(epoch),
        applied_at_block_id: BlockId::zero(),
        applied_at_unix_secs: 1_700_000_100,
    }
}

#[test]
fn save_and_get_roundtrip() {
    let (db, _tmp) = create_rocksdb();
    let directive = sample_directive(1);
    let record = applied_record(&directive, 5);

    db.with_write_tx(|tx| AppliedDirective::save(&record, tx)).unwrap();

    let fetched = db
        .with_read_tx(|tx| AppliedDirective::get(tx, &directive.id()))
        .unwrap();
    assert_eq!(fetched.directive_id, directive.id());
    assert_eq!(fetched.applied_at_epoch, Epoch(5));
    assert_eq!(fetched.body.nonce, 1);
}

#[test]
fn get_nonexistent_returns_not_found() {
    let (db, _tmp) = create_rocksdb();
    let directive = sample_directive(99);

    let result = db.with_read_tx(|tx| AppliedDirective::get(tx, &directive.id()).optional());
    assert!(matches!(result, Ok(None)));
}

#[test]
fn save_is_idempotent_overwrite() {
    let (db, _tmp) = create_rocksdb();
    let directive = sample_directive(2);
    let mut record = applied_record(&directive, 3);
    db.with_write_tx(|tx| AppliedDirective::save(&record, tx)).unwrap();

    // Overwriting with a newer epoch — last write wins. (Orchestrator is expected to dedupe
    // before saving; this test documents the storage-level behaviour.)
    record.applied_at_epoch = Epoch(7);
    db.with_write_tx(|tx| AppliedDirective::save(&record, tx)).unwrap();

    let fetched = db
        .with_read_tx(|tx| AppliedDirective::get(tx, &directive.id()))
        .unwrap();
    assert_eq!(fetched.applied_at_epoch, Epoch(7));
}

#[test]
fn distinct_directives_do_not_collide() {
    let (db, _tmp) = create_rocksdb();
    let d_a = sample_directive(10);
    let d_b = sample_directive(11);
    assert_ne!(d_a.id(), d_b.id(), "different nonce must yield different ID");

    db.with_write_tx(|tx| {
        AppliedDirective::save(&applied_record(&d_a, 1), tx)?;
        AppliedDirective::save(&applied_record(&d_b, 2), tx)?;
        Ok::<_, StorageError>(())
    })
    .unwrap();

    let a = db.with_read_tx(|tx| AppliedDirective::get(tx, &d_a.id())).unwrap();
    let b = db.with_read_tx(|tx| AppliedDirective::get(tx, &d_b.id())).unwrap();
    assert_eq!(a.applied_at_epoch, Epoch(1));
    assert_eq!(b.applied_at_epoch, Epoch(2));
}
