//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Guards the per-transaction Wasmer metering budget (`MAX_WASM_POINTS_PER_TRANSACTION`). Each
//! template invocation gets a fresh per-call budget, so without a transaction-wide cap a transaction
//! could run for far longer than any single call by stacking instructions. These tests prove the
//! total is capped across calls.

use tari_engine_types::{commit_result::RejectReason, limits::MAX_WASM_POINTS_PER_TRANSACTION};
use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const METERING_BENCH: &str = "tests/templates/metering_bench";

#[test]
fn per_transaction_budget_caps_total_across_calls() {
    let mut test = TemplateTest::new(CRATE_PATH, [METERING_BENCH]);
    let addr = test.get_template_address("MeteringBench");
    let (account, owner, key) = test.create_funded_account();

    test.enable_fees();

    let call = |rounds: u64, n: usize| {
        let mut builder = Transaction::builder_localnet(Epoch(1)).pay_fee_from_component(account, 900_000_000u64);
        for _ in 0..n {
            builder = builder.call_function(addr, "bench_div_u64", args![rounds]);
        }
        builder.build_and_seal(&key)
    };

    // Calibrate points per round so we can size a call to a known fraction of the budget.
    let mut points = |rounds: u64| -> u64 {
        test.execute_expect_success(call(rounds, 1), vec![owner.clone()])
            .wasm_execution_points
    };
    let per_round = (points(20_000) - points(10_000)) / 10_000;

    // Size one call to ~65% of the budget: comfortably under it on its own, but two such calls in a
    // single transaction sum to ~130% and must exceed it.
    let rounds = MAX_WASM_POINTS_PER_TRANSACTION * 65 / 100 / per_round;

    // One call stays under the budget and succeeds.
    test.execute_expect_success(call(rounds, 1), vec![owner.clone()]);

    // Two identical calls exceed the transaction-wide budget: the second runs out of gas even
    // though, on a fresh per-call budget, it would have succeeded.
    let reason = test.execute_expect_failure(call(rounds, 2), vec![owner.clone()]);
    assert!(
        matches!(reason, RejectReason::ExecutionFailure(_)),
        "expected an out-of-gas execution failure, got {reason:?}",
    );
}
