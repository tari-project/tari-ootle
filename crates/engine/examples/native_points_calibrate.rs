//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Calibrates the [`NativeExecutionPoints`] constants (`crates/engine_types/src/limits.rs`) that
//! price native, unmetered crypto verification in Wasmer metering points.
//!
//! Native verification (stealth transfers, confidential withdraws, burn claims) runs outside the
//! WASM meter, so its point price is derived by wall-clock equivalence: measure how many metering
//! points one millisecond of real WASM execution charges, measure how many milliseconds each native
//! verification op costs, and multiply.
//!
//! # The premise this exists to test
//!
//! That multiplication is only sound if the WASM:native *ratio* is stable across validator classes
//! — each side's absolute speed varies with hardware, but the constants are consensus-wide, so a
//! single number has to be right everywhere. The premise is plausible (both sides are CPU-bound) but
//! not self-evident: curve arithmetic and WASM interpretation lean on different parts of a
//! microarchitecture, and `curve25519-dalek`'s AVX2 backend does not track a WASM interpreter's
//! branch-prediction and cache behaviour. Run this on at least two microarchitectures and compare;
//! `--compare` reports whether the points-equivalents held even as the raw timings moved.
//!
//! # Two sets of constants, because there are two jobs
//!
//! The same measurement feeds two consumers with opposite error preferences:
//!
//! * **bound** — sizing [`FREE_COMPUTE_GRACE_POINTS`] and the pre-charge that stops a non-paying transaction extracting
//!   verification work. Over-estimating is safe; under-estimating is not. Uses the worst pairwise marginal.
//! * **price** — what a paying user is charged. Over-estimating is a permanent tax on every honest transaction. Uses
//!   the amortised marginal across the measured range, which reproduces the measured cost exactly at both endpoints.
//!
//! Reporting only the conservative figure, as this tool previously did, silently applies a safety
//! margin to a price.
//!
//! # Running
//!
//! ```text
//! cargo run -p tari_engine --example native_points_calibrate --release
//! cargo run -p tari_engine --example native_points_calibrate --release -- --json --label m1-max > m1.json
//! cargo run -p tari_engine --example native_points_calibrate --release -- --compare m1.json --label c6i.2xlarge
//! ```
//!
//! In `--json` mode stdout is the JSON document and all progress goes to stderr, so it redirects
//! cleanly. The raw per-`n` timings are included so any derived figure can be re-checked without
//! re-running.
//!
//! Methodology mirrors `metering_recost`: two round counts per measurement and a slope, which
//! cancels fixed per-call overhead (transaction assembly, fee intent, account loads); wall-clock is
//! best-of-N to suppress scheduling noise, while point counts are deterministic (read from the
//! `WasmExecution` fee breakdown at a 1-point = 1-fee-unit rate). For distribution statistics on the
//! native side alone, see the `stealth_cost` criterion bench.

use std::{collections::BTreeMap, time::Instant};

use serde::{Deserialize, Serialize};
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
const MAX_FEE: u64 = 60_000_000;
/// Trials per engine execution. Each is a full transaction, so these are the expensive samples — but
/// the rate they produce multiplies every constant, so it is the last place to economise. The run's
/// noise check covers these samples for that reason.
const ENGINE_TRIALS: usize = 11;
/// Trials per native verification. `validate_transfer` costs on the order of a millisecond, so a
/// large sample is cheap — and necessary: at five trials the per-output marginal moved by ~2x
/// between runs on a lightly loaded machine, which silently moves every constant derived from it.
const NATIVE_TRIALS: usize = 100;

/// Output counts the per-output marginal is fitted over. Powers of two so an aggregated bulletproof
/// never pays a padding step mid-range.
const OUTPUT_COUNTS: [usize; 4] = [1, 2, 4, 8];
/// Input counts for the per-input marginal.
const INPUT_COUNTS: [usize; 2] = [100, 1000];

/// Metering points per microtari under the committed fee schedule (`per_wasm_point_cost: 1`,
/// `wasm_points_cost_divisor: 1000` — see `fee_tables.rs`). Used only to express the constants as a
/// user-facing transfer cost; it is not an input to any of them.
const POINTS_PER_MICROTARI: f64 = 1000.0;

/// A burn claim verifies one Schnorr ownership proof plus commitment arithmetic (the same primitives
/// as the fixed per-transfer balance-proof check) and a bounded kernel-MMR inclusion proof (Blake2b
/// hashes, microseconds), so it prices as the fixed transfer cost with headroom.
const CLAIM_BURN_HEADROOM: f64 = 1.5;

/// Median-vs-min spread above which even the minimum is likely contaminated and the run should be
/// discarded. Deliberately loose: dispersion grows with how long a single measurement runs, because
/// a longer one is likelier to be interrupted, so the eight-output samples always spread wider than
/// the one-output sample on the same machine. That is expected and does not invalidate the minimum,
/// which is the estimator the constants are fitted on.
const NOISE_THRESHOLD: f64 = 0.5;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Host {
    label: String,
    os: String,
    arch: String,
    cpus: usize,
    debug_assertions: bool,
}

/// Marginal costs in milliseconds, each derived twice — see the module docs on bound vs price.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StealthMs {
    /// Worst pairwise marginal across [`OUTPUT_COUNTS`].
    per_output_bound: f64,
    /// Amortised marginal across the whole measured range.
    per_output_price: f64,
    vk_surcharge_bound: f64,
    vk_surcharge_price: f64,
    per_input: f64,
    /// Per-statement remainder, `t(1) - per_output`, so each model reproduces `t(1)` exactly.
    fixed_bound: f64,
    fixed_price: f64,
}

/// One timed verification. `min` is what the slopes are fitted on — scheduling noise only ever adds
/// time, so the fastest observation is the closest estimate of the real cost. `median` is carried
/// alongside purely so a noisy run is visible: on a quiet machine the two sit close together, and a
/// wide gap means the run should be discarded rather than trusted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Sample {
    min_ms: f64,
    median_ms: f64,
}

impl Sample {
    /// Spread as a fraction of the minimum; a rough noise indicator for the run.
    fn dispersion(&self) -> f64 {
        if self.min_ms == 0.0 {
            return 0.0;
        }
        (self.median_ms - self.min_ms) / self.min_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawMs {
    /// Engine executions keyed by round count; the WASM rate is the slope between them.
    wasm_rounds: BTreeMap<u64, Sample>,
    outputs_no_view_key: BTreeMap<usize, Sample>,
    outputs_view_key: BTreeMap<usize, Sample>,
    inputs: BTreeMap<usize, Sample>,
}

impl RawMs {
    fn worst_dispersion(&self) -> f64 {
        self.wasm_rounds
            .values()
            .chain(self.outputs_no_view_key.values())
            .chain(self.outputs_view_key.values())
            .chain(self.inputs.values())
            .map(Sample::dispersion)
            .fold(0.0, f64::max)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Constants {
    per_statement: u64,
    per_output: u64,
    per_output_viewable_surcharge: u64,
    per_input: u64,
    per_claim_burn: u64,
}

impl Constants {
    /// Points for the canonical shape: one stealth input, two stealth outputs (recipient plus
    /// change), on a resource with no view key.
    fn transfer_1in_2out(&self) -> u64 {
        self.per_statement + 2 * self.per_output + self.per_input
    }

    /// The most expensive legitimate fee-sourcing flow: one transfer with a single stealth change
    /// output and up to 64 dust inputs. The fee amount itself is a revealed output and costs no
    /// proof, and fees are TARI, which never carries a view key.
    fn stealth_fee_source(&self) -> u64 {
        self.per_statement + self.per_output + 64 * self.per_input
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Calibration {
    host: Host,
    wasm_rate_points_per_ms: f64,
    stealth_ms: StealthMs,
    raw_ms: RawMs,
    /// Conservative: for the grace and the pre-charge. Conservative in the *total* for a statement
    /// of any size, not field by field — the fixed term is the `t(1)` remainder, so a larger
    /// per-output marginal leaves a smaller `PER_STATEMENT`. Both models agree exactly at one
    /// output and the bound exceeds the price above that.
    bound: Constants,
    /// Realistic: for what a paying transaction is charged.
    price: Constants,
}

struct Args {
    json: bool,
    label: String,
    compare: Option<String>,
}

fn parse_args() -> Args {
    let mut json = false;
    let mut label = String::new();
    let mut compare = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--label" => label = it.next().unwrap_or_default(),
            "--compare" => compare = it.next(),
            "--help" | "-h" => {
                eprintln!(
                    "usage: native_points_calibrate [--json] [--label NAME] [--compare BASELINE.json]\n\n  \
                     --json              emit the calibration as JSON on stdout (progress goes to stderr)\n  \
                     --label NAME        tag this run (e.g. the machine class) so comparisons are readable\n  \
                     --compare FILE      diff this run against a baseline JSON written by an earlier --json run"
                );
                std::process::exit(0);
            },
            other => {
                eprintln!("unrecognised argument {other:?}; try --help");
                std::process::exit(2);
            },
        }
    }
    Args { json, label, compare }
}

fn host(label: String) -> Host {
    Host {
        label: if label.is_empty() {
            format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
        } else {
            label
        },
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        debug_assertions: cfg!(debug_assertions),
    }
}

fn main() {
    let args = parse_args();
    if cfg!(debug_assertions) {
        eprintln!("WARNING: not a release build — timings are meaningless. Re-run with --release.");
    }

    let calibration = measure(host(args.label));

    if let Some(path) = &args.compare {
        let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read baseline {path}: {e}"));
        let baseline: Calibration =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("cannot parse baseline {path}: {e}"));
        report_comparison(&baseline, &calibration);
    }

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&calibration).expect("Calibration is serialisable")
        );
    } else {
        report(&calibration);
    }
}

fn measure(host: Host) -> Calibration {
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

    // One measured execution: returns (WasmExecution points, wall-clock sample in ms).
    let run = |test: &mut TemplateTest, rounds: u64| -> (u64, Sample) {
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
        // Warm up, then sample; points are deterministic per round count.
        let _ = execute(test);
        let mut points = 0;
        let mut ms: Vec<f64> = Vec::with_capacity(ENGINE_TRIALS);
        for _ in 0..ENGINE_TRIALS {
            let (p, ns) = execute(test);
            points = p;
            ms.push(ns / 1e6);
        }
        ms.sort_by(f64::total_cmp);
        (points, Sample {
            min_ms: ms[0],
            median_ms: ms[ms.len() / 2],
        })
    };

    let (points_r1, sample_r1) = run(&mut test, R1);
    let (points_r2, sample_r2) = run(&mut test, R2);
    let point_slope = (points_r2 - points_r1) as f64;
    let ms_slope = sample_r2.min_ms - sample_r1.min_ms;
    let wasm_rate_points_per_ms = point_slope / ms_slope;
    eprintln!(
        "WASM rate: {wasm_rate_points_per_ms:.0} points/ms  (marginal {} points over {ms_slope:.3} ms)",
        point_slope as u64,
    );

    let (stealth_ms, mut raw_ms) = measure_stealth_costs();
    // The rate multiplies every constant, so its noise is included in the run's quality check.
    raw_ms.wasm_rounds = [(R1, sample_r1), (R2, sample_r2)].into_iter().collect();

    let to_points = |ms: f64| -> u64 { (ms * wasm_rate_points_per_ms).ceil() as u64 };
    let bound = Constants {
        per_statement: to_points(stealth_ms.fixed_bound),
        per_output: to_points(stealth_ms.per_output_bound),
        per_output_viewable_surcharge: to_points(stealth_ms.vk_surcharge_bound),
        per_input: to_points(stealth_ms.per_input),
        per_claim_burn: to_points(stealth_ms.fixed_bound * CLAIM_BURN_HEADROOM),
    };
    let price = Constants {
        per_statement: to_points(stealth_ms.fixed_price),
        per_output: to_points(stealth_ms.per_output_price),
        per_output_viewable_surcharge: to_points(stealth_ms.vk_surcharge_price),
        per_input: to_points(stealth_ms.per_input),
        per_claim_burn: to_points(stealth_ms.fixed_price * CLAIM_BURN_HEADROOM),
    };

    Calibration {
        host,
        wasm_rate_points_per_ms,
        stealth_ms,
        raw_ms,
        bound,
        price,
    }
}

/// Times `validate_transfer` directly (no engine) across [`OUTPUT_COUNTS`] and [`INPUT_COUNTS`].
fn measure_stealth_costs() -> (StealthMs, RawMs) {
    let view_key = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(7u64));

    let time_validate = |stmt: &StealthTransferStatement, vk: Option<&RistrettoPublicKey>| -> Sample {
        validate_transfer(stmt, vk).expect("statement must verify");
        let mut ms: Vec<f64> = (0..NATIVE_TRIALS)
            .map(|_| {
                let start = Instant::now();
                drop(std::hint::black_box(validate_transfer(std::hint::black_box(stmt), vk)).expect("verifies"));
                start.elapsed().as_nanos() as f64 / 1e6
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        Sample {
            min_ms: ms[0],
            median_ms: ms[ms.len() / 2],
        }
    };

    // Output scaling, with and without a view key: the base per-output price is the aggregated
    // bulletproof share; a view key adds one ElGamal viewable-balance proof per output.
    let sweep = |vk: Option<&RistrettoPublicKey>| -> BTreeMap<usize, Sample> {
        let measured: BTreeMap<usize, Sample> = OUTPUT_COUNTS
            .into_iter()
            .map(|n| {
                let data = generate_mint_statement(vec![1_000u64; n], 0u64, vk);
                (n, time_validate(&data.statement, vk))
            })
            .collect();
        for (n, s) in &measured {
            eprintln!(
                "stealth outputs ({}) n={n}: {:.4} ms (median {:.4}, spread {:+.1}%)",
                if vk.is_some() { "view key" } else { "no view key" },
                s.min_ms,
                s.median_ms,
                s.dispersion() * 100.0,
            );
        }
        measured
    };

    /// Worst pairwise marginal — the constant then covers the steepest step rather than the
    /// amortised average, which is what a safety bound needs.
    fn bound_slope(measured: &BTreeMap<usize, Sample>) -> f64 {
        measured
            .iter()
            .zip(measured.iter().skip(1))
            .map(|((n0, t0), (n1, t1))| (t1.min_ms - t0.min_ms) / (*n1 - *n0) as f64)
            .fold(f64::MIN, f64::max)
    }

    /// Amortised marginal across the whole range. Combined with `t(1) - slope` as the fixed term
    /// this reproduces the measured cost exactly at both endpoints, so it neither over- nor
    /// under-charges a transfer of either size.
    fn price_slope(measured: &BTreeMap<usize, Sample>) -> f64 {
        let (n_lo, t_lo) = measured.iter().next().expect("non-empty sweep");
        let (n_hi, t_hi) = measured.iter().next_back().expect("non-empty sweep");
        (t_hi.min_ms - t_lo.min_ms) / (*n_hi - *n_lo) as f64
    }

    let no_vk = sweep(None);
    let with_vk = sweep(Some(&view_key));

    let t1_novk = no_vk.values().next().expect("non-empty sweep").min_ms;
    let per_output_bound = bound_slope(&no_vk);
    let per_output_price = price_slope(&no_vk);

    // Input scaling: per-input commitment decompress + point add (plus substate work charged by the
    // engine separately). Linear in the input count, so one slope serves both models.
    let inputs_stmt = |n: usize| -> StealthTransferStatement {
        let inputs = (0..n)
            .map(|i| MaskAndValue {
                mask: RistrettoSecretKey::from(i as u64 + 1),
                value: 1,
            })
            .collect::<Vec<_>>();
        generate_transfer_data(inputs, 0u64, vec![n as u64], 0u64).statement
    };
    let inputs: BTreeMap<usize, Sample> = INPUT_COUNTS
        .into_iter()
        .map(|n| (n, time_validate(&inputs_stmt(n), None)))
        .collect();
    for (n, s) in &inputs {
        eprintln!(
            "stealth inputs n={n}: {:.4} ms (median {:.4}, spread {:+.1}%)",
            s.min_ms,
            s.median_ms,
            s.dispersion() * 100.0,
        );
    }
    let per_input = price_slope(&inputs);

    let stealth_ms = StealthMs {
        per_output_bound,
        per_output_price,
        vk_surcharge_bound: (bound_slope(&with_vk) - per_output_bound).max(0.0),
        vk_surcharge_price: (price_slope(&with_vk) - per_output_price).max(0.0),
        per_input,
        fixed_bound: (t1_novk - per_output_bound).max(0.0),
        fixed_price: (t1_novk - per_output_price).max(0.0),
    };
    let raw_ms = RawMs {
        // Filled in by the caller, which owns the engine measurements.
        wasm_rounds: BTreeMap::new(),
        outputs_no_view_key: no_vk,
        outputs_view_key: with_vk,
        inputs,
    };
    (stealth_ms, raw_ms)
}

fn microtari(points: u64) -> f64 {
    points as f64 / POINTS_PER_MICROTARI
}

fn report(c: &Calibration) {
    let h = &c.host;
    println!();
    println!(
        "host: {} ({} {}, {} cpus)   WASM rate: {:.0} points/ms",
        h.label, h.arch, h.os, h.cpus, c.wasm_rate_points_per_ms
    );
    let noise = c.raw_ms.worst_dispersion();
    if noise > NOISE_THRESHOLD {
        println!(
            "WARNING: worst median-vs-min spread {:.0}% exceeds {:.0}% — at that level the minimum is\n         \
             likely contaminated too. Discard this run and repeat on an idle machine.",
            noise * 100.0,
            NOISE_THRESHOLD * 100.0,
        );
    } else {
        println!(
            "noise: worst median-vs-min spread {:.0}% (grows with sample duration; the minimum is the estimator)",
            noise * 100.0
        );
    }
    println!();
    println!("                                 bound (grace)      price (charge)");
    let row = |name: &str, b: u64, p: u64| println!("  {name:<30} {b:>12}      {p:>12}");
    row("PER_STATEMENT", c.bound.per_statement, c.price.per_statement);
    row("PER_OUTPUT", c.bound.per_output, c.price.per_output);
    row(
        "PER_OUTPUT_VIEWABLE_SURCHARGE",
        c.bound.per_output_viewable_surcharge,
        c.price.per_output_viewable_surcharge,
    );
    row("PER_INPUT", c.bound.per_input, c.price.per_input);
    row("PER_CLAIM_BURN", c.bound.per_claim_burn, c.price.per_claim_burn);
    println!();
    println!(
        "  stealth transfer (1 in, 2 out) {:>12}      {:>12} points",
        c.bound.transfer_1in_2out(),
        c.price.transfer_1in_2out(),
    );
    println!(
        "                                 {:>11.0}µT      {:>11.0}µT  at 1µT/1000 points",
        microtari(c.bound.transfer_1in_2out()),
        microtari(c.price.transfer_1in_2out()),
    );
    println!();
    println!(
        "  FREE_COMPUTE_GRACE_POINTS >= {} (2x the {} of the stealth fee-sourcing flow)",
        c.bound.stealth_fee_source() * 2,
        c.bound.stealth_fee_source(),
    );
    println!();
    println!(
        "The bound column is conservative in the total, not field by field: the fixed term is the\none-output \
         remainder, so a larger per-output marginal leaves a smaller PER_STATEMENT. The\ntwo models agree exactly at \
         one output and the bound exceeds the price above that.\nPER_VALUE_PROOF is not measured here — it has no \
         harness yet."
    );
}

/// Reports whether the WASM:native ratio held between two machines. Absolute timings are expected to
/// differ; the points-equivalents are what the consensus constants fix, so a large delta in the
/// points column is the finding — it means one constant cannot be correct for both.
fn report_comparison(baseline: &Calibration, current: &Calibration) {
    let pct = |from: f64, to: f64| -> String {
        if from == 0.0 {
            return "     n/a".to_string();
        }
        format!("{:+7.1}%", (to - from) / from * 100.0)
    };

    println!();
    println!(
        "Comparison: baseline {:?} vs current {:?}",
        baseline.host.label, current.host.label
    );
    println!(
        "  {} {} / {} cpus   ->   {} {} / {} cpus",
        baseline.host.arch, baseline.host.os, baseline.host.cpus, current.host.arch, current.host.os, current.host.cpus
    );
    println!();
    println!("                              baseline        current       delta");
    println!(
        "  WASM rate (points/ms)  {:>13.0}  {:>13.0}   {}",
        baseline.wasm_rate_points_per_ms,
        current.wasm_rate_points_per_ms,
        pct(baseline.wasm_rate_points_per_ms, current.wasm_rate_points_per_ms),
    );
    println!();
    println!("  Native wall-clock (ms) — expected to differ with hardware:");
    let ms_row = |name: &str, b: f64, c: f64| println!("    {name:<24} {b:>11.5}  {c:>13.5}   {}", pct(b, c));
    ms_row(
        "per-output",
        baseline.stealth_ms.per_output_price,
        current.stealth_ms.per_output_price,
    );
    ms_row("fixed", baseline.stealth_ms.fixed_price, current.stealth_ms.fixed_price);
    ms_row("per-input", baseline.stealth_ms.per_input, current.stealth_ms.per_input);
    println!();
    println!("  Points-equivalent (price model) — THIS is what must hold:");
    let pt_row = |name: &str, b: u64, c: u64| println!("    {name:<24} {b:>11}  {c:>13}   {}", pct(b as f64, c as f64));
    pt_row(
        "PER_STATEMENT",
        baseline.price.per_statement,
        current.price.per_statement,
    );
    pt_row("PER_OUTPUT", baseline.price.per_output, current.price.per_output);
    pt_row(
        "PER_OUTPUT_VIEWABLE_SUR..",
        baseline.price.per_output_viewable_surcharge,
        current.price.per_output_viewable_surcharge,
    );
    pt_row("PER_INPUT", baseline.price.per_input, current.price.per_input);
    pt_row(
        "transfer (1 in, 2 out)",
        baseline.price.transfer_1in_2out(),
        current.price.transfer_1in_2out(),
    );
    println!();
    println!(
        "  Read the transfer total, not the individual terms. PER_STATEMENT is the one-output\n  remainder t(1) - \
         PER_OUTPUT, so the two absorb each other's slope noise and each swings\n  far wider than their sum: on a \
         single machine they move by tens of percent between\n  runs while the total holds to a few. Only the total \
         is usable at this sample size."
    );
    println!();
    println!(
        "  A large delta in the transfer total falsifies the ratio-stability premise the\n  constants rest on: no \
         single value is then correct for both machines, and they must be\n  set from the slowest-native/fastest-WASM \
         corner of the hardware envelope rather than\n  measured on one box. Repeat each side a few times first — \
         this tool's own run-to-run\n  spread on a quiet machine is the noise floor a cross-machine delta has to \
         clear."
    );
    println!();
}
