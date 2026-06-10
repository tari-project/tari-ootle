//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A template designed to consume as much execution time as possible per call without tripping the
//! Wasmer metering limit (currently 100_000_000 points per `_main` invocation, see
//! `tari_engine::wasm::module`). It is used to stress test transaction execution and consensus
//! throughput under worst-case CPU load.
//!
//! The hot loop is a fully serial chain of 64-bit integer divisions. Division is the cheapest
//! operation to meter (1 point, the same as an add) yet one of the most expensive to actually
//! execute (tens of CPU cycles, with no pipelining when each result feeds the next), so it
//! maximises wall-clock time per metered point. Every divisor is forced odd, so the divide can
//! never trap on a zero divisor and the call always completes successfully.

use tari_template_lib::prelude::*;

/// Inner divisions per outer iteration. A constant bound lets the optimiser unroll the inner loop,
/// amortising the outer-loop bookkeeping so a larger fraction of the metering budget is spent on
/// the divisions themselves.
const DIVISIONS_PER_ROUND: u32 = 8;

/// Odd multiplier (a 64-bit prime-ish constant). Multiplying by an odd value is a bijection modulo
/// 2^64, which keeps the accumulator spread across the full 64 bits so every divisor stays wide and
/// each divide keeps hitting the hardware slow path.
const MIX: u64 = 0x9E37_79B9_7F4A_7C17;

/// Number of outer rounds that lands just under the 100M metering budget. Calibrated empirically
/// (see `tests/max_compute.rs` in the `transaction_generator` crate): the measured cost is ~110
/// points per round, so this targets ~88M points and leaves ~12% head-room under the 100M cap for
/// compiler/metering drift.
const MAX_ROUNDS: u64 = 800_000;

#[template]
mod max_compute {
    use super::*;

    pub struct MaxCompute {}

    impl MaxCompute {
        /// Run `rounds` outer iterations of the division grinder and return the accumulator so the
        /// work cannot be optimised away. Callers are responsible for keeping `rounds` at or below
        /// the metering budget; see [`busy_max`](Self::busy_max) for the calibrated maximum.
        pub fn busy(rounds: u64) -> u64 {
            grind(rounds)
        }

        /// Run the calibrated maximum number of rounds: as much compute as a single call can do
        /// without exhausting the metering budget.
        pub fn busy_max() -> u64 {
            grind(MAX_ROUNDS)
        }
    }
}

/// The grinder: a serial dependent chain of 64-bit divisions.
fn grind(rounds: u64) -> u64 {
    let mut acc: u64 = 0xD1B5_4A32_D192_ED03;
    let mut i: u64 = 0;
    while i < rounds {
        let mut j: u32 = 0;
        while j < DIVISIONS_PER_ROUND {
            // Divisor is the top 16 bits forced odd: small enough that the quotient is wide (the
            // slow path for a 64-bit divide) and never zero (so the divide never traps). The
            // quotient feeds straight into the next divisor, so the divides cannot overlap.
            let divisor = (acc >> 48) | 1;
            acc = acc.wrapping_div(divisor).wrapping_mul(MIX);
            j = j.wrapping_add(1);
        }
        // Fold in the outer counter so the optimiser cannot prove the accumulator settles into a
        // short cycle and short-circuit the loop.
        acc ^= i;
        i = i.wrapping_add(1);
    }
    acc
}
