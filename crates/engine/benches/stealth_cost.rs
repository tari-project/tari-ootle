//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Measures the native crypto cost of verifying a single stealth transfer (`validate_transfer`) as a function of the
//! number of inputs and outputs, with and without a resource view key (ElGamal viewable-balance proofs). Used to
//! calibrate the per-transaction stealth caps and the transaction-weight pricing for stealth transfers.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::stealth::validate_transfer;
use tari_template_lib::types::stealth::StealthTransferStatement;
use tari_template_test_tooling::{
    support::stealth::{generate_mint_statement, generate_transfer_data},
    wallet_crypto::MaskAndValue,
};

fn view_key() -> RistrettoPublicKey {
    RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(7u64))
}

/// A transfer of `n` stealth outputs and no stealth inputs (a revealed-funded mint). Exercises the aggregated
/// bulletproof over `n` outputs plus, with a view key, `n` ElGamal proofs.
fn outputs_statement(n: usize, with_view_key: bool) -> (StealthTransferStatement, Option<RistrettoPublicKey>) {
    let vk = with_view_key.then(view_key);
    let data = generate_mint_statement(vec![1_000u64; n], 0u64, vk.as_ref());
    (data.statement, vk)
}

/// A transfer spending `n` stealth inputs into a single output. Exercises the per-input commitment aggregation
/// (point decompress + add) plus one bulletproof and one balance proof.
fn inputs_statement(n: usize) -> StealthTransferStatement {
    let inputs = (0..n)
        .map(|i| MaskAndValue {
            mask: RistrettoSecretKey::from(i as u64 + 1),
            value: 1,
        })
        .collect::<Vec<_>>();
    generate_transfer_data(inputs, 0u64, vec![n as u64], 0u64).statement
}

fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("stealth_validate_transfer");

    // Output scaling, no view key.
    for n in [1usize, 2, 4, 8] {
        let (stmt, vk) = outputs_statement(n, false);
        g.bench_with_input(BenchmarkId::new("outputs", n), &n, |b, _| {
            b.iter(|| black_box(validate_transfer(black_box(&stmt), vk.as_ref())).unwrap())
        });
    }

    // Output scaling, with view key (adds one ElGamal viewable-balance proof per output).
    for n in [1usize, 2, 4, 8] {
        let (stmt, vk) = outputs_statement(n, true);
        g.bench_with_input(BenchmarkId::new("outputs_viewkey", n), &n, |b, _| {
            b.iter(|| black_box(validate_transfer(black_box(&stmt), vk.as_ref())).unwrap())
        });
    }

    // Input scaling, single output, no view key.
    for n in [1usize, 10, 100, 1000] {
        let stmt = inputs_statement(n);
        g.bench_with_input(BenchmarkId::new("inputs", n), &n, |b, _| {
            b.iter(|| black_box(validate_transfer(black_box(&stmt), None)).unwrap())
        });
    }

    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
