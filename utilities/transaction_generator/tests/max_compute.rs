//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Guards the `max_compute` stress template (templates/max_compute). `busy_max()` is meant to run
//! the largest amount of metered work it can while staying safely under the Wasmer metering limit;
//! these tests prove it succeeds, pin how close to the limit it runs, and let `MAX_ROUNDS` be
//! retuned if the cost drifts.

use tari_engine_types::fees::FeeSource;
use tari_ootle_transaction::{Transaction, args};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const MAX_COMPUTE: &str = "templates/max_compute";

/// The per-call Wasmer metering budget (see `tari_engine::wasm::module::create_engine`).
const METERING_LIMIT: u64 = 100_000_000;

/// `busy_max()` must run as much metered work as possible without exhausting its budget: if a
/// toolchain change pushed the cost over the cap the call would fail with an out-of-gas trap, so
/// success here proves it stays under, and the bounds pin how close it runs.
#[test]
fn busy_max_stays_under_the_metering_budget() {
    let mut test = TemplateTest::new(CRATE_PATH, [MAX_COMPUTE]);
    let (account, owner, key) = test.create_funded_account();
    let addr = test.get_template_address("MaxCompute");

    // 1 fee unit == 1 metering point, so the WasmExecution charge is the exact point count.
    let mut fee_table = test.fee_table().clone();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();

    // Succeeds => the single busy_max() call did not exhaust its metering budget.
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, 900_000_000u64)
            .call_function(addr, "busy_max", args![])
            .build_and_seal(&key),
        vec![owner],
    );
    let points = wasm_points(&result.finalize);

    // The whole transaction's metered points (busy_max plus the account fee call) must fit under a
    // single call's budget — busy_max alone is the dominant term and the only one that can grow.
    assert!(
        points < METERING_LIMIT,
        "busy_max consumed {points} points, at/over the {METERING_LIMIT} metering limit; lower MAX_ROUNDS",
    );
    // And it must actually be doing heavy work — guard against the loop being optimised away.
    assert!(
        points > METERING_LIMIT * 3 / 4,
        "busy_max consumed only {points} points; expected close to the {METERING_LIMIT} budget",
    );
}

/// The cost is linear in `rounds`, which is what lets `MAX_ROUNDS` be calibrated. This pins the
/// slope (~358 points/round with division re-costed to 30 points); a change here means the template
/// or the metering table was retuned and `MAX_ROUNDS` should be rechecked.
#[test]
fn busy_cost_is_linear_in_rounds() {
    let mut test = TemplateTest::new(CRATE_PATH, [MAX_COMPUTE]);
    let (account, owner, key) = test.create_funded_account();
    let addr = test.get_template_address("MaxCompute");

    let mut fee_table = test.fee_table().clone();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();

    let mut busy = |rounds: u64| {
        let result = test.execute_expect_success(
            Transaction::builder_localnet()
                .pay_fee_from_component(account, 900_000_000u64)
                .call_function(addr, "busy", args![rounds])
                .build_and_seal(&key),
            vec![owner.clone()],
        );
        wasm_points(&result.finalize)
    };

    let p1 = busy(20_000);
    let p2 = busy(40_000);

    let per_round = (p2 - p1) / 20_000;
    assert!(
        (330..=390).contains(&per_round),
        "expected ~358 metering points per round, measured {per_round}",
    );
}

fn wasm_points(finalize: &tari_engine_types::commit_result::FinalizeResult) -> u64 {
    finalize
        .fee_receipt
        .fee_breakdown()
        .iter()
        .find_map(|(s, a)| (*s == FeeSource::WasmExecution).then_some(*a))
        .expect("WasmExecution charge present")
}
