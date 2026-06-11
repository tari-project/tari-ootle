//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Measures the real per-op execution cost of WASM operators through the engine's actual
//! compile+execute path, to inform re-costing `crates/engine/src/wasm/metering.rs`.
//!
//! Run on the hardware you want to price for (ideally the *slowest* validator you support — division
//! is far more expensive relative to an add on x86 than on Apple Silicon):
//!
//!     cargo run -p tari_engine --example metering_recost --release
//!
//! Methodology: each `bench_*` function in the `metering_bench` template runs a serial dependent
//! chain with identical per-step glue, varying only one measured op. For each op we time the call at
//! two round counts and take the slope, which cancels fixed per-call overhead, giving time per step.
//! `bench_cheap8` (8 cheap ops) and `bench_noop` (glue only) calibrate the cost of a single cheap op
//! (a single one is below the noise floor), and every op is priced as round(its latency / one
//! cheap-op's latency). Latency (dependent chain), not throughput, is what a busy-loop attacker
//! realises, so it's the conservative number to price.

use std::time::Instant;

use tari_ootle_transaction::args;
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE: &str = "tests/templates/metering_bench";

/// Must match `INNER` in the template (measured ops per outer round).
const INNER: f64 = 64.0;
/// Round counts for the two-point slope. Kept under the 100M per-call metering cap for the heaviest
/// bench (`bench_cheap8`, ~16 points/step: 70k * 64 * 16 ≈ 72M points).
const R1: u64 = 35_000;
const R2: u64 = 70_000;
const TRIALS: usize = 9;

struct Bench {
    label: &'static str,
    func: &'static str,
    current: u64,
    float: bool,
}

const fn b(label: &'static str, func: &'static str, current: u64, float: bool) -> Bench {
    Bench {
        label,
        func,
        current,
        float,
    }
}

fn main() {
    let benches = [
        b("i64.and (ref)", "bench_and", 1, false),
        b("i64.add", "bench_add", 1, false),
        b("i64.sub", "bench_sub", 1, false),
        b("i64.or", "bench_or", 1, false),
        b("i64.xor", "bench_xor", 1, false),
        b("i64.shl", "bench_shl", 1, false),
        b("i64.mul", "bench_mul", 1, false),
        b("i64.div_u", "bench_div_u64", 1, false),
        b("i64.div_s", "bench_div_s64", 1, false),
        b("i64.rem_u", "bench_rem_u64", 1, false),
        b("i32.div_u", "bench_div_u32", 1, false),
        b("f64.add", "bench_fadd", 4, true),
        b("f64.mul", "bench_fmul", 4, true),
        b("f64.div", "bench_fdiv", 4, true),
    ];

    eprintln!("Compiling metering_bench template and warming up...");
    let mut test = TemplateTest::new(CRATE_PATH, [TEMPLATE]);

    // ns per measured step (op + identical glue) for each named function.
    let step_ns = |test: &mut TemplateTest, func: &str| -> f64 {
        let time = |test: &mut TemplateTest, rounds: u64| -> f64 {
            // Warm up, then take the fastest of TRIALS to suppress scheduling noise.
            let _: u64 = test.call_function("MeteringBench", func, args![rounds], vec![]);
            let mut best = f64::MAX;
            for _ in 0..TRIALS {
                let start = Instant::now();
                let _: u64 = test.call_function("MeteringBench", func, args![rounds], vec![]);
                best = best.min(start.elapsed().as_nanos() as f64);
            }
            best
        };
        let slope = (time(test, R2) - time(test, R1)) / (R2 - R1) as f64;
        slope / INNER
    };

    let noop = step_ns(&mut test, "bench_noop");
    let fnoop = step_ns(&mut test, "bench_fnoop");
    // `bench_cheap8` and `bench_noop` share identical glue and differ by exactly 8 cheap ops, so
    // their step-time difference ÷ 8 is the cost of one cheap (~1-cycle) op — the "1 point" unit. A
    // single cheap op is below the measurement noise floor, hence the ×8 chain.
    let cheap_op_ns = (step_ns(&mut test, "bench_cheap8") - noop) / 8.0;

    println!();
    println!("Per-op cost (lower bound; dependent-chain latency). 1 cheap op ≈ {cheap_op_ns:.3} ns");
    println!();
    println!(
        "{:<14} {:>10} {:>10} {:>9} {:>10}",
        "op", "ns/op", "x cheap", "current", "suggested"
    );
    println!("{}", "-".repeat(57));

    for bench in &benches {
        let step = step_ns(&mut test, bench.func);
        // Isolate the op's latency from the shared glue by subtracting the matching glue-only
        // reference (float ops use the float glue, integer ops the integer glue). Clamp at zero:
        // ops costing ~1 cheap op are below the timing noise floor and can read slightly negative.
        let op_ns = (if bench.float { step - fnoop } else { step - noop }).max(0.0);
        let ratio = op_ns / cheap_op_ns;
        let suggested = ratio.round().max(1.0) as u64;
        println!(
            "{:<14} {:>10.3} {:>10.1} {:>9} {:>10}",
            bench.label, op_ns, ratio, bench.current, suggested
        );
    }

    println!();
    println!("Notes:");
    println!("- Ops at ~1x are below the single-op timing noise floor; they are priced at the 1-point");
    println!("  minimum. The signal is reliable for the expensive ops (div/rem/mul/float).");
    println!("- Run on your slowest validator class: division's ratio is much higher on x86 (~30-90");
    println!("  cycles) than on Apple Silicon (~2-9), so re-cost against the worst case.");
    println!("- Numbers are dependent-chain latency (worst case a busy loop can realise), the");
    println!("  conservative figure to price against.");
    println!("- f64.sqrt and memory/cache costs are not covered here and need separate measurement.");
}
