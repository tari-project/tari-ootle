//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Frame `i` of a generated file is the transaction built for index `i`.
//!
//! File position `i` is builder index `i`, so any prefix of the file covers accounts `0..k`. The
//! builder for a smaller index is delayed here so it finishes last.

use std::{
    io::{Cursor, Seek, SeekFrom},
    thread,
    time::Duration,
};

use tari_ootle_transaction::{Epoch, Network, Transaction, TransactionId};
use transaction_generator::{read_transactions, transaction_builders::free_coins, write_transactions};

const NUM_TRANSACTIONS: u64 = 8;
const SEED: u64 = 7;

fn expected_ids() -> Vec<TransactionId> {
    let build = free_coins::seeded_builder(Network::LocalNet, Epoch(1), SEED);
    (0..NUM_TRANSACTIONS).map(|i| build(i).calculate_id()).collect()
}

fn write_file() -> Cursor<Vec<u8>> {
    let build = free_coins::seeded_builder(Network::LocalNet, Epoch(1), SEED);
    let mut buf = Cursor::new(Vec::new());
    write_transactions(
        NUM_TRANSACTIONS,
        Box::new(move |i| {
            // Smaller indexes take longer, so a pool that writes on completion emits them last.
            thread::sleep(Duration::from_millis((NUM_TRANSACTIONS - i) * 20));
            build(i)
        }),
        &|_| {},
        &mut buf,
    )
    .unwrap();
    buf.seek(SeekFrom::Start(0)).unwrap();
    buf
}

#[test]
fn transactions_are_written_in_index_order_even_when_later_indexes_finish_first() {
    let ids: Vec<TransactionId> = read_transactions(write_file(), 0)
        .unwrap()
        .into_iter()
        .map(|tx: Transaction| tx.calculate_id())
        .collect();

    assert_eq!(ids, expected_ids());
}
