//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Calibrates the `NATIVE_POINTS_*` constants (`crates/engine_types/src/limits.rs`) that price
//! native, unmetered crypto verification in Wasmer metering points.
//!
//! Native verification (stealth transfers, confidential withdraws, burn claims) runs outside the
//! WASM meter, so its point price must be derived from wall-clock equivalence: measure how many
//! metering points one millisecond of real WASM execution charges, measure how many milliseconds
//! each native verification op costs, and multiply. Both sides are CPU-bound on the same hardware,
//! so the *ratio* is stable across validator classes even though each side's absolute time is not.
//!
//! Run on the hardware you want to price for:
//!
//!     cargo run -p tari_engine --example native_points_calibrate --release
//!
//! Methodology mirrors `metering_recost`: two round counts per measurement and a slope, which
//! cancels fixed per-call overhead (transaction assembly, fee intent, account loads); wall-clock is
//! best-of-N to suppress scheduling noise, while point counts are deterministic (read from the
//! `WasmExecution` fee breakdown at a 1-point = 1-fee-unit rate).

use std::time::Instant;

use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine::fees::FeeTable;
use tari_engine_types::{fees::FeeSource, stealth::validate_transfer};
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::stealth::StealthTransferStatement;
use tari_template_test_tooling::{
    TemplateTest,
    support::stealth::{generate_mint_statement, generate_transfer_data},
    wallet_crypto::MaskAndValue,
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const METERING_BENCH: &str = "tests/templates/metering_bench";

/// Two-point slope round counts. Sized so the heavier run stays well under the per-call metering
/// cap while the marginal wasm work dominates the fixed transaction overhead. Every run pays its
/// real fee out of the funded account (~2.8k points/round at the recosted div price), so the round
/// counts and `MAX_FEE` are also sized to keep the whole calibration within the account's funding.
const R1: u64 = 5_000;
const R2: u64 = 10_000;
const TRIALS: usize = 5;
const MAX_FEE: u64 = 60_000_000;

fn main() {
    eprintln!("Compiling metering_bench template and warming up...");
    let mut test = TemplateTest::new(CRATE_PATH, [METERING_BENCH]);
    let bench = test.get_template_address("MeteringBench");
    let (account, owner, key) = test.create_funded_account();

    // Price WASM execution at 1 fee unit per metering point so the WasmExecution charge *is* the
    // point count.
    let mut fee_table = FeeTable::zero_rated();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();

    // One measured execution: returns (WasmExecution points, best-of-TRIALS wall-clock ns).
    let run = |test: &mut TemplateTest, rounds: u64| -> (u64, f64) {
        let execute = |test: &mut TemplateTest| -> (u64, f64) {
            let tx = Transaction::builder_localnet()
                .pay_fee_from_component(account, MAX_FEE)
                .call_function(bench, "bench_div_u64", args![rounds])
                .build_and_seal(&key);
            let start = Instant::now();
            let result = test.execute_expect_success(tx, vec![owner.clone()]);
            let elapsed = start.elapsed().as_nanos() as f64;
            let points = result
                .finalize
                .fee_receipt
                .fee_breakdown()
                .iter()
                .find_map(|(s, a)| (*s == FeeSource::WasmExecution).then_some(*a))
                .expect("WasmExecution charge present");
            (points, elapsed)
        };
        // Warm up, then best-of-TRIALS for time; points are deterministic per round count.
        let _ = execute(test);
        let mut best_ns = f64::MAX;
        let mut points = 0;
        for _ in 0..TRIALS {
            let (p, ns) = execute(test);
            points = p;
            best_ns = best_ns.min(ns);
        }
        (points, best_ns)
    };

    let (points_r1, ns_r1) = run(&mut test, R1);
    let (points_r2, ns_r2) = run(&mut test, R2);
    let point_slope = (points_r2 - points_r1) as f64;
    let ns_slope = ns_r2 - ns_r1;
    let points_per_ms = point_slope / (ns_slope / 1e6);
    println!();
    println!(
        "WASM rate: {:.0} points/ms  (marginal {} points over {:.3} ms)",
        points_per_ms,
        point_slope as u64,
        ns_slope / 1e6
    );

    let (per_output_ms, vk_surcharge_ms, per_input_ms, fixed_ms) = measure_stealth_costs();

    // ---- Suggested constants ----

    let to_points = |ms: f64| -> u64 { (ms * points_per_ms).ceil() as u64 };
    println!();
    println!(
        "Measured: per-output {per_output_ms:.4} ms (+{vk_surcharge_ms:.4} ms with view key), per-input \
         {per_input_ms:.6} ms, fixed {fixed_ms:.4} ms"
    );
    println!();
    println!("Suggested constants (round up to taste):");
    println!("  PER_OUTPUT                    = {}", to_points(per_output_ms));
    println!("  PER_OUTPUT_VIEWABLE_SURCHARGE = {}", to_points(vk_surcharge_ms));
    println!("  PER_INPUT                     = {}", to_points(per_input_ms));
    println!("  PER_STATEMENT                 = {}", to_points(fixed_ms));
    // A burn claim verifies one Schnorr ownership proof plus commitment arithmetic (the same
    // primitives as the fixed per-transfer balance-proof check) and a bounded kernel-MMR inclusion
    // proof (Blake2b hashes, microseconds). Price it as the fixed transfer cost with headroom.
    println!("  PER_CLAIM_BURN                = {}", to_points(fixed_ms * 1.5));
    println!();
    // The grace must fit the most expensive legitimate fee-sourcing flow. With native metering, a
    // stealth-UTXO-funded fee (1 transfer: fixed cost + 1 stealth change output + up to 64 dust
    // inputs; the fee amount itself is a revealed output, which costs no proof) becomes the worst
    // case, ahead of the ~143k-point AMM swap. Fees are TARI, which never carries a view key, so
    // the flow prices at the base output rate.
    let stealth_fee_source = to_points(fixed_ms + per_output_ms + 64.0 * per_input_ms);
    println!("Legit stealth fee-source (1 transfer, 1 output no-vk, 64 inputs) = {stealth_fee_source} points");
    println!(
        "Suggested FREE_COMPUTE_GRACE_POINTS >= {} (2x margin, and >= 2x the 143k AMM-swap flow)",
        stealth_fee_source * 2
    );
}

/// Times `validate_transfer` directly (no engine) and derives (per-output ms without a view key,
/// view-key surcharge ms per output, per-input ms, fixed-per-transfer ms).
fn measure_stealth_costs() -> (f64, f64, f64, f64) {
    let view_key = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(7u64));

    let time_validate = |stmt: &StealthTransferStatement, vk: Option<&RistrettoPublicKey>| -> f64 {
        validate_transfer(stmt, vk).expect("statement must verify");
        let mut best = f64::MAX;
        for _ in 0..TRIALS {
            let start = Instant::now();
            drop(std::hint::black_box(validate_transfer(std::hint::black_box(stmt), vk)).expect("verifies"));
            best = best.min(start.elapsed().as_nanos() as f64);
        }
        best / 1e6 // ms
    };

    // Output scaling, with and without a view key: the base per-output price is the aggregated
    // bulletproof share; a view key adds one ElGamal viewable-balance proof per output.
    let marginal = |vk: Option<&RistrettoPublicKey>| -> (f64, f64) {
        let out_ms: Vec<(usize, f64)> = [1usize, 2, 4, 8]
            .into_iter()
            .map(|n| {
                let data = generate_mint_statement(vec![1_000u64; n], 0u64, vk);
                (n, time_validate(&data.statement, vk))
            })
            .collect();
        for (n, ms) in &out_ms {
            println!(
                "stealth outputs ({}) n={n}: {ms:.3} ms",
                if vk.is_some() { "view key" } else { "no view key" }
            );
        }
        // Marginal per-output cost: max pairwise slope, so the constant covers the worst marginal
        // rather than the amortised average of large aggregations.
        let per_output = out_ms
            .windows(2)
            .map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0) as f64)
            .fold(f64::MIN, f64::max);
        (per_output, out_ms[0].1)
    };
    let (per_output_ms, t1_novk) = marginal(None);
    let (per_output_vk_ms, _) = marginal(Some(&view_key));
    let vk_surcharge_ms = (per_output_vk_ms - per_output_ms).max(0.0);
    // Fixed per-transfer cost: balance proof + bulletproof base + basic validations.
    let fixed_ms = (t1_novk - per_output_ms).max(0.0);

    // Input scaling: per-input commitment decompress + point add (plus substate work charged by
    // the engine separately).
    let inputs_stmt = |n: usize| -> StealthTransferStatement {
        let inputs = (0..n)
            .map(|i| MaskAndValue {
                mask: RistrettoSecretKey::from(i as u64 + 1),
                value: 1,
            })
            .collect::<Vec<_>>();
        generate_transfer_data(inputs, 0u64, vec![n as u64], 0u64).statement
    };
    let t_in_small = time_validate(&inputs_stmt(100), None);
    let t_in_large = time_validate(&inputs_stmt(1000), None);
    let per_input_ms = (t_in_large - t_in_small) / 900.0;
    println!("stealth inputs: 100 -> {t_in_small:.3} ms, 1000 -> {t_in_large:.3} ms");

    (per_output_ms, vk_surcharge_ms, per_input_ms, fixed_ms)
}
