//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Microbenchmark template for re-costing the Wasmer metering table (`crates/engine/src/wasm/
//! metering.rs`). The metering cost function assigns points per WASM operator, but those points
//! only approximate execution time if they track real op latency — today many slow ops (notably
//! integer division) are priced the same as an add. The `metering_recost` example
//! (`crates/engine/examples/metering_recost.rs`) calls these functions through the real engine and
//! derives a measured cost per op.
//!
//! Every `bench_*` function runs the same loop: a serial dependent chain where each step computes a
//! small non-zero operand, applies exactly one measured op, and folds the result back with a fixed
//! cheap "mix". Keeping the per-step glue identical across functions means the difference between
//! two functions' per-step time is the difference in their measured op, so each op can be priced
//! relative to a cheap reference (`bench_and`/`bench_noop`).

use tari_template_lib::prelude::*;

/// Measured ops per outer round. A constant bound lets the optimiser unroll the inner loop so the
/// outer-loop bookkeeping is amortised and the per-step time is dominated by the chain itself.
pub const INNER: u32 = 64;

const SEED: u64 = 0xD1B5_4A32_D192_ED03;

/// Odd 64-bit multiplier. Multiplying by it is a bijection mod 2^64, so it keeps the accumulator
/// spread across all 64 bits regardless of how narrow the measured op makes its result (a remainder
/// is always < divisor) — without this the chain collapses and div/rem degenerate to a trivial path.
const MIX: u64 = 0x9E37_79B9_7F4A_7C17;

#[template]
mod metering_bench {
    use super::*;

    pub struct MeteringBench {}

    impl MeteringBench {
        // Reference points -------------------------------------------------------------------
        /// Just the mix, no operand op (the operand is dead and elided). Lower bound per step.
        pub fn bench_noop(rounds: u64) -> u64 {
            grind(rounds, |acc, _d| acc)
        }
        /// Cheap 1-cycle op in the slot. Used as the "1 point" reference.
        pub fn bench_and(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc & d)
        }
        /// Eight dependent cheap ops (4 add + 4 xor, non-folding). A single cheap op is below the
        /// measurement noise floor, so the per-cheap-op unit is derived from this ÷ 8.
        pub fn bench_cheap8(rounds: u64) -> u64 {
            grind(rounds, |acc, _d| {
                let mut x = acc;
                x = x.wrapping_add(0x1234_5678_9ABC_DEF1);
                x ^= 0xF0E1_D2C3_B4A5_9687;
                x = x.wrapping_add(0x0F1E_2D3C_4B5A_6978);
                x ^= 0x8796_A5B4_C3D2_E1F0;
                x = x.wrapping_add(0xDEAD_BEEF_CAFE_F00D);
                x ^= 0x0123_4567_89AB_CDEF;
                x = x.wrapping_add(0xFEDC_BA98_7654_3210);
                x ^= 0xA5A5_5A5A_C3C3_3C3C;
                x
            })
        }

        // Integer arithmetic / logic --------------------------------------------------------
        pub fn bench_add(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc.wrapping_add(d))
        }
        pub fn bench_sub(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc.wrapping_sub(d))
        }
        pub fn bench_or(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc | d)
        }
        pub fn bench_xor(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc ^ d)
        }
        pub fn bench_shl(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc << (d & 63))
        }
        pub fn bench_mul(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc.wrapping_mul(d))
        }

        // The targets: integer division / remainder ----------------------------------------
        pub fn bench_div_u64(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc.wrapping_div(d))
        }
        pub fn bench_rem_u64(rounds: u64) -> u64 {
            grind(rounds, |acc, d| acc.wrapping_rem(d))
        }
        pub fn bench_div_s64(rounds: u64) -> u64 {
            // d is provably in 1..=0xFFFF (positive, never 0 or -1), so this lowers to a bare
            // i64.div_s with no overflow/zero guards.
            grind(rounds, |acc, d| ((acc as i64) / (d as i64)) as u64)
        }
        pub fn bench_div_u32(rounds: u64) -> u64 {
            grind(rounds, |acc, d| ((acc as u32) / (d as u32)) as u64)
        }

        // Floating point (f64.add/mul/div lower to native WASM ops; sqrt needs std, omitted) -
        pub fn bench_fadd(rounds: u64) -> u64 {
            grind_f(rounds, |f| f + 1.000_001)
        }
        pub fn bench_fmul(rounds: u64) -> u64 {
            grind_f(rounds, |f| f * 1.000_000_3)
        }
        pub fn bench_fdiv(rounds: u64) -> u64 {
            grind_f(rounds, |f| f / 1.000_000_7)
        }
        pub fn bench_fnoop(rounds: u64) -> u64 {
            grind_f(rounds, |f| f)
        }
    }
}

/// Integer dependent-chain grinder. `op(acc, d)` is the one measured op. `d` is taken from the low
/// 16 bits (always populated, in 1..=0xFFFF) so division has a wide dividend / wide quotient and
/// never traps. The mix folds the op result back, mixes in `d` (so `d` is never dead, even for the
/// no-op reference), and multiplies by `MIX` to keep `acc` full-width so nothing degenerates. The
/// per-step glue is identical for every op, so step-time differences isolate the op.
fn grind(rounds: u64, op: impl Fn(u64, u64) -> u64) -> u64 {
    let mut acc: u64 = SEED;
    let mut i: u64 = 0;
    while i < rounds {
        let mut j: u32 = 0;
        while j < INNER {
            let d = (acc & 0xFFFF) | 1;
            let v = op(acc, d);
            acc = (v ^ d).rotate_left(1).wrapping_mul(MIX);
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    acc
}

/// Float dependent-chain grinder. The contracting mix keeps `f` positive and bounded so divides and
/// the chain stay on the normal (non-denormal, non-inf) path.
fn grind_f(rounds: u64, op: impl Fn(f64) -> f64) -> u64 {
    let mut f: f64 = 1.0001;
    let mut i: u64 = 0;
    while i < rounds {
        let mut j: u32 = 0;
        while j < INNER {
            let v = op(f);
            f = v * 0.9999 + 1.0;
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    f.to_bits()
}
