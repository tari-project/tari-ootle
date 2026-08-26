//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Measures what a Groth16/BN254 verifier costs when implemented entirely in a WASM template
//! (`crates/engine/tests/templates/zk_verifier`), against the per-transaction metering budget
//! ([`MAX_WASM_POINTS_PER_TRANSACTION`]).
//!
//! This is the "no engine support" baseline: the number a native verification op has to beat, and
//! the evidence the per-transaction ceiling is sized against. `tests/zk_verify.rs` asserts the
//! bounds; this prints the breakdown they come from. Run with:
//!
//! ```text
//! cargo run -p tari_engine --release --example zk_points_calibrate
//! ```
//!
//! Every WASM figure is exact rather than sampled: metering points are deterministic, so one
//! execution per measurement is the whole answer. Only the millisecond, fee and native columns are
//! derived — via the calibrated rate in [`POINTS_PER_MS`], the shipped fee schedule, and timing.

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::limits::{MAX_WASM_POINTS_PER_TRANSACTION, NativeExecutionPoints};
use tari_ootle_transaction::{Epoch, Transaction, args, builder::named_args::NamedArg};
use tari_template_test_tooling::TemplateTest;

#[path = "../tests/support/groth16.rs"]
mod groth16;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const ZK_VERIFIER: &str = "tests/templates/zk_verifier";

/// Metering points per millisecond of validator CPU, from `examples/native_points_calibrate.rs`.
/// Converts point counts into the wall-clock terms the block budget is reasoned about in.
const POINTS_PER_MS: f64 = 8_400_000.0;

/// Metering points per microtari under the shipped fee schedule (`per_wasm_point_cost: 1`,
/// `wasm_points_cost_divisor: 1000` — see `applications/tari_ootle_app_utilities/src/fee_tables.rs`).
const POINTS_PER_MICROTARI: f64 = 1000.0;

/// Public input counts the per-input marginal is fitted over. The verifier's only input-dependent
/// work is the `gamma_abc_g1` MSM, which is linear in this.
const PUBLIC_INPUT_COUNTS: [usize; 4] = [1, 4, 16, 64];

/// Public input count the single-shot breakdown is measured at: a realistic contract statement (a
/// root, a nullifier, a recipient, an amount).
const BREAKDOWN_INPUTS: usize = 4;

/// Trials per native measurement. Verification is on the order of a millisecond, so a large sample
/// is cheap and the minimum it yields is far less contaminated than a small one's.
const NATIVE_TRIALS: usize = 200;

/// Runs `count` identical calls in one transaction and returns the WASM metering points the whole
/// transaction consumed, or `None` if it ran out of gas — the answer for anything above the
/// ceiling, and the case this example exists to detect. Fees stay disabled so the calls get the
/// full per-transaction budget rather than an allowance bounded by what they have paid; the
/// question is what the work costs, not what funds it.
fn measure_stacked(
    test: &mut TemplateTest,
    key: &RistrettoSecretKey,
    func: &str,
    args: impl Fn() -> Vec<NamedArg>,
    count: usize,
) -> Option<u64> {
    let addr = test.get_template_address("ZkVerifier");
    let transaction = Transaction::builder_localnet(Epoch(1))
        .fold(0..count, |builder, _| builder.call_function(addr, func, args()))
        .build_and_seal(key);
    let result = test.try_execute(transaction, vec![]).expect("execution");
    result
        .finalize
        .any_reject()
        .is_none()
        .then_some(result.wasm_execution_points)
}

/// Runs one template call. See [`measure_stacked`].
fn measure(test: &mut TemplateTest, key: &RistrettoSecretKey, func: &str, args: Vec<NamedArg>) -> Option<u64> {
    measure_stacked(test, key, func, || args.clone(), 1)
}

#[expect(clippy::too_many_lines)]
fn main() {
    let mut test = TemplateTest::new(CRATE_PATH, [ZK_VERIFIER]);
    let (_account, _owner, key) = test.create_funded_account();
    let f = groth16::fixture(BREAKDOWN_INPUTS);

    println!("Groth16/BN254 verification in a WASM template");
    println!(
        "  budget: {} points/transaction ({:.1} ms at {:.1}M points/ms)\n",
        MAX_WASM_POINTS_PER_TRANSACTION,
        MAX_WASM_POINTS_PER_TRANSACTION as f64 / POINTS_PER_MS,
        POINTS_PER_MS / 1e6,
    );

    println!("encoded sizes (bytes, {BREAKDOWN_INPUTS} public inputs)");
    println!(
        "  vk   compressed {:>6}  uncompressed {:>6}",
        f.vk_compressed.len(),
        f.vk_uncompressed.len()
    );
    println!(
        "  pvk                        uncompressed {:>6}",
        f.pvk_uncompressed.len()
    );
    println!(
        "  proof compressed {:>5}  uncompressed {:>6}",
        f.proof_compressed.len(),
        f.proof_uncompressed.len()
    );
    println!("  inputs {:>4}\n", f.inputs.len());

    let baseline = measure(&mut test, &key, "noop", args![f.proof_uncompressed.clone()]).expect("noop fits");

    let row = |test: &mut TemplateTest, label: &str, func: &str, args: Vec<NamedArg>| {
        let Some(total) = measure(test, &key, func, args) else {
            println!("  {label:<34} {:>12}  out of gas — exceeds the budget", "");
            return None;
        };
        let net = total.saturating_sub(baseline);
        println!(
            "  {label:<34} {net:>12} pts  {:>7.2} ms  {:>9.0} uT  {:>6.1}% of budget",
            net as f64 / POINTS_PER_MS,
            net as f64 / POINTS_PER_MICROTARI,
            100.0 * net as f64 / MAX_WASM_POINTS_PER_TRANSACTION as f64,
        );
        Some(net)
    };

    println!("breakdown ({BREAKDOWN_INPUTS} public inputs, net of {baseline} pts call overhead)");
    row(
        &mut test,
        "decode proof (compressed)",
        "decode_proof_compressed",
        args![f.proof_compressed.clone()],
    );
    row(
        &mut test,
        "decode proof (uncompressed)",
        "decode_proof_uncompressed",
        args![f.proof_uncompressed.clone()],
    );
    row(
        &mut test,
        "decode proof (no subgroup check)",
        "decode_proof_uncompressed_unchecked",
        args![f.proof_uncompressed.clone()],
    );
    row(&mut test, "decode vk (uncompressed)", "decode_vk_uncompressed", args![
        f.vk_uncompressed.clone()
    ]);
    row(&mut test, "prepare vk", "prepare", args![f.vk_uncompressed.clone()]);
    println!();

    println!("full verify ({BREAKDOWN_INPUTS} public inputs)");
    let compressed = row(&mut test, "vk compressed, per call", "verify_compressed", args![
        f.vk_compressed.clone(),
        f.proof_compressed.clone(),
        f.inputs.clone()
    ]);
    let uncompressed = row(&mut test, "vk uncompressed, per call", "verify_uncompressed", args![
        f.vk_uncompressed.clone(),
        f.proof_uncompressed.clone(),
        f.inputs.clone()
    ]);
    let prepared = row(&mut test, "pvk uncompressed, per call", "verify_prepared", args![
        f.pvk_uncompressed.clone(),
        f.proof_uncompressed.clone(),
        f.inputs.clone()
    ]);
    println!();

    println!("public input scaling (verify_prepared, net of overhead)");
    let mut points_by_inputs = Vec::new();
    for n in PUBLIC_INPUT_COUNTS {
        let fx = groth16::fixture(n);
        let Some(total) = measure(&mut test, &key, "verify_prepared", args![
            fx.pvk_uncompressed,
            fx.proof_uncompressed,
            fx.inputs
        ]) else {
            println!("  {n:>3} inputs               out of gas — exceeds the budget");
            continue;
        };
        let net = total.saturating_sub(baseline);
        println!(
            "  {n:>3} inputs {net:>12} pts  {:>7.2} ms  {:>6.1}% of budget",
            net as f64 / POINTS_PER_MS,
            100.0 * net as f64 / MAX_WASM_POINTS_PER_TRANSACTION as f64,
        );
        points_by_inputs.push((n, net));
    }
    if let (Some(&(n_lo, p_lo)), Some(&(n_hi, p_hi))) = (points_by_inputs.first(), points_by_inputs.last()) &&
        n_hi > n_lo
    {
        let per_input = p_hi.saturating_sub(p_lo) as f64 / (n_hi - n_lo) as f64;
        println!(
            "  marginal {per_input:.0} pts/input  ({:.4} ms)",
            per_input / POINTS_PER_MS
        );
    }
    println!();

    // Each invocation would get a fresh per-call budget were the transaction-wide cap not enforced,
    // so this is where that cap becomes visible: the count that fits is set by the budget, not by
    // how the calls are split across instructions.
    println!("stacked verifies in one transaction ({BREAKDOWN_INPUTS} public inputs)");
    let mut stacked_fit = 0;
    for count in 1.. {
        let args = || {
            args![
                f.pvk_uncompressed.clone(),
                f.proof_uncompressed.clone(),
                f.inputs.clone()
            ]
        };
        let Some(total) = measure_stacked(&mut test, &key, "verify_prepared", args, count) else {
            println!("  {count:>3} verifies             out of gas — exceeds the budget");
            break;
        };
        println!(
            "  {count:>3} verifies {total:>12} pts  {:>7.2} ms  {:>6.1}% of budget",
            total as f64 / POINTS_PER_MS,
            100.0 * total as f64 / MAX_WASM_POINTS_PER_TRANSACTION as f64,
        );
        stacked_fit = count;
    }
    println!();

    println!("native reference (same proofs, verified in-process)");
    for n in PUBLIC_INPUT_COUNTS {
        let ms = groth16::native_verify_ms(n, NATIVE_TRIALS);
        println!(
            "  {n:>3} inputs {:>12.0} pts  {ms:>7.2} ms  {:>9.0} uT",
            ms * POINTS_PER_MS,
            ms * POINTS_PER_MS / POINTS_PER_MICROTARI,
        );
    }
    println!();

    println!("verdict");
    match prepared {
        Some(prepared) => {
            println!(
                "  cheapest in-WASM verify is {:.1}% of the per-transaction budget; {stacked_fit} fit in one \
                 transaction",
                100.0 * prepared as f64 / MAX_WASM_POINTS_PER_TRANSACTION as f64,
            );
            if let (Some(compressed), Some(uncompressed)) = (compressed, uncompressed) {
                println!(
                    "  compression costs {:.1}x; per-call vk preparation costs {:.1}x over a stored pvk",
                    compressed as f64 / prepared.max(1) as f64,
                    uncompressed as f64 / prepared.max(1) as f64,
                );
            }
        },
        None => println!(
            "  no in-WASM verify fits the {MAX_WASM_POINTS_PER_TRANSACTION} point per-transaction budget; raise the \
             ceiling to measure by how much"
        ),
    }
    println!(
        "  for scale, one stealth output (native bulletproof) is priced at {} pts",
        NativeExecutionPoints::PER_OUTPUT,
    );
}
