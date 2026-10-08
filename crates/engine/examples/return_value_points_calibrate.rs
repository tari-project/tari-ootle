//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Prices the host-side handling of a template call's return value in metering points.
//!
//! A template function returns up to `max_call_size` bytes of CBOR, which the engine counts, decodes
//! into an `IndexedValue`, indexes for well-known types, clones into the instruction output and
//! encodes again for whoever made the call. None of it runs under the Wasmer meter, and its cost
//! follows the number of CBOR items far more than the number of bytes: a byte string is one item
//! however long, a run of one-byte integers is an item a byte.
//!
//! Each shape below fills `max_call_size` with the cheapest-to-encode item of its kind and times
//! that whole sequence, so the worst of them bounds the per-item price. The per-byte price is the
//! one-item byte string, which is nothing but copies. Both are converted to points at the same WASM
//! points-per-millisecond rate `native_points_calibrate` derives, and by the same method, so the
//! figures are comparable.
//!
//! `validate_return_value` is left out: it needs a running transaction's working state, and its
//! work is one map or store-cache lookup per bucket, proof and substate the value references, each
//! looked up twice (once in `WasmProcess::invoke`, once in the `TransactionProcessor` call that ran
//! it).
//! Every such reference is a tag and its payload, at least two items, so the per-item price covers
//! those lookups with the decode work they come with.
//!
//! Run with `--release`; a debug build measures nothing useful.

use std::{hint::black_box, time::Instant};

use tari_engine::fees::FeeTable;
use tari_engine_types::{
    fees::FeeSource,
    indexed_value::IndexedValue,
    limits::{self, NativeExecutionPoints},
};
use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const METERING_BENCH: &str = "tests/templates/metering_bench";

/// Times each shape is handled; the fastest is kept.
const TRIALS: usize = 50;
/// Executions timed per round count when deriving the WASM rate.
const ENGINE_TRIALS: usize = 7;
/// Round counts for the WASM rate fit.
const R1: u64 = 5_000;
const R2: u64 = 10_000;
const MAX_FEE: u64 = 60_000_000;

/// The CBOR head of a definite-length array or map of `len` entries.
fn head(major: u8, len: usize) -> Vec<u8> {
    let mut out = vec![major << 5 | 26];
    out.extend_from_slice(&u32::try_from(len).unwrap().to_be_bytes());
    out
}

/// An array filling `max_call_size` with copies of `item`.
fn array_of(item: &[u8]) -> Vec<u8> {
    let len = (limits::ENGINE_LIMITS.max_call_size - 5) / item.len();
    let mut out = head(4, len);
    out.extend(item.iter().copied().cycle().take(len * item.len()));
    out
}

fn shapes() -> Vec<(&'static str, Vec<u8>)> {
    let max = limits::ENGINE_LIMITS.max_call_size;
    let mut map = head(5, (max - 5) / 2);
    map.extend(std::iter::repeat_n(0x00, (max - 5) / 2 * 2));
    let mut chunked = vec![0x5f];
    chunked.extend(std::iter::repeat_n(0x40, max - 2));
    chunked.push(0xff);
    let mut bytes = head(2, max - 5);
    bytes.extend(std::iter::repeat_n(0xab, max - 5));

    vec![
        ("[0, 0, ..]", array_of(&[0x00])),
        ("[null, null, ..]", array_of(&[0xf6])),
        ("[[], [], ..]", array_of(&[0x80])),
        ("[{}, {}, ..]", array_of(&[0xa0])),
        ("[[[0]], [[0]], ..]", array_of(&[0x81, 0x81, 0x00])),
        ("[\"\", \"\", ..]", array_of(&[0x60])),
        ("[\"a\", \"a\", ..]", array_of(&[0x61, 0x61])),
        ("[h'00', h'00', ..]", array_of(&[0x41, 0x00])),
        ("{0: 0, ..}", map),
        ("[0(0), 0(0), ..]", array_of(&[0xc0, 0x00])),
        ("[bucket(0), ..]", array_of(&[0xd8, 0x85, 0x00])),
        ("(_ h'', h'', ..)", chunked),
        ("h'abab..'", bytes),
    ]
}

/// What the engine does with a returned value, fastest of [`TRIALS`], in milliseconds.
fn handle_ms(raw: &[u8]) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..TRIALS {
        let start = Instant::now();
        let items = tari_bor::count_data_items(raw).expect("well-formed");
        let value = IndexedValue::from_raw(raw).expect("decodes");
        let output = value.clone();
        let reencoded = tari_bor::encode(value.value()).expect("encodes");
        let elapsed = start.elapsed().as_nanos() as f64 / 1e6;
        black_box((items, output, reencoded));
        best = best.min(elapsed);
    }
    best
}

/// The points-per-millisecond rate, derived exactly as `native_points_calibrate` derives it: two
/// round counts of the same metered loop, fitted on the marginal points over the marginal time.
fn wasm_rate_points_per_ms() -> f64 {
    let mut test = TemplateTest::new(CRATE_PATH, [METERING_BENCH]);
    let bench = test.get_template_address("MeteringBench");
    let (account, owner, key) = test.create_funded_account();

    let mut fee_table = FeeTable::zero_rated();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();

    let mut run = |rounds: u64| -> (u64, f64) {
        let execute = |test: &mut TemplateTest| -> (u64, f64) {
            let tx = Transaction::builder_localnet(Epoch(1))
                .pay_fee_from_component(account, MAX_FEE)
                .call_function(bench, "bench_div_u64", args![rounds])
                .build_and_seal(&key);
            let start = Instant::now();
            let result = test.execute_expect_success(tx, vec![owner.clone()]);
            let elapsed = start.elapsed().as_nanos() as f64 / 1e6;
            let points = result
                .finalize
                .fee_receipt
                .fee_breakdown()
                .iter()
                .find_map(|(s, a)| (*s == FeeSource::WasmExecution).then_some(*a))
                .expect("WasmExecution charge present");
            (points, elapsed)
        };
        let _ = execute(&mut test);
        let mut points = 0;
        let mut ms = Vec::with_capacity(ENGINE_TRIALS);
        for _ in 0..ENGINE_TRIALS {
            let (p, t) = execute(&mut test);
            points = p;
            ms.push(t);
        }
        ms.sort_by(f64::total_cmp);
        (points, ms[0])
    };

    let (points_r1, ms_r1) = run(R1);
    let (points_r2, ms_r2) = run(R2);
    (points_r2 - points_r1) as f64 / (ms_r2 - ms_r1)
}

fn main() {
    let rate = wasm_rate_points_per_ms();
    println!("WASM rate: {rate:.0} points/ms\n");
    println!(
        "{:<22} {:>8} {:>8} {:>10} {:>12} {:>10}",
        "shape", "bytes", "items", "ms", "points/item", "charged/ms"
    );
    for (label, raw) in shapes() {
        let items = tari_bor::count_data_items(&raw).expect("well-formed");
        let ms = handle_ms(&raw);
        let points = ms * rate;
        let charged = limits::return_value_points(raw.len() as u64, items);
        println!(
            "{label:<22} {:>8} {items:>8} {ms:>10.3} {:>12.1} {:>9.2}x",
            raw.len(),
            points / items as f64,
            charged as f64 / points,
        );
    }
    println!(
        "\nPER_RETURN_VALUE_ITEM = {}, PER_RETURN_VALUE_BYTE = {}. The last column is what those charge over what was \
         measured.",
        NativeExecutionPoints::PER_RETURN_VALUE_ITEM,
        NativeExecutionPoints::PER_RETURN_VALUE_BYTE,
    );
}
