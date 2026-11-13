//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Write;

use bytes::{BufMut, Bytes, BytesMut};
use rayon::prelude::*;
use tari_ootle_transaction::Transaction;

use crate::BoxedTransactionBuilder;

/// How many transactions are built at once.
///
/// An indexed parallel collect returns a chunk in index order, and chunks are written in sequence,
/// so file position `i` is builder index `i`. Any prefix of a seeded file therefore covers accounts
/// `0..k`: a faucet file that is only partly submitted, or resumed with `--skip`, has funded those
/// accounts, which is the range a transfers flood of length `k` reads.
const BUILD_CHUNK: usize = 1024;

pub fn write_transactions<W: Write>(
    num_transactions: u64,
    builder: BoxedTransactionBuilder,
    on_progress: &dyn Fn(usize),
    writer: &mut W,
) -> anyhow::Result<()> {
    writer.write_all(&num_transactions.to_le_bytes())?;

    let mut written = 0usize;
    let mut start = 0u64;
    while start < num_transactions {
        let chunk = usize::try_from(num_transactions - start)
            .unwrap_or(usize::MAX)
            .min(BUILD_CHUNK);
        // `0..chunk` is an indexed parallel iterator, so `collect` places frame `offset` at
        // `frames[offset]` however the builds finish.
        let frames: Vec<Bytes> = (0..chunk)
            .into_par_iter()
            .map(|offset| encode_frame(&builder(start + offset as u64)))
            .collect();
        for frame in frames {
            writer.write_all(&frame)?;
            written += 1;
            if written.is_multiple_of(10_000) {
                on_progress(written);
            }
        }
        start += chunk as u64;
    }

    Ok(())
}

fn encode_frame(transaction: &Transaction) -> Bytes {
    let buf = tari_bor::encode(transaction).unwrap();
    let len = u16::try_from(buf.len()).unwrap().to_le_bytes();
    let mut output = BytesMut::with_capacity(buf.len() + len.len());
    output.put_slice(&len);
    output.put_slice(&buf);
    output.freeze()
}
