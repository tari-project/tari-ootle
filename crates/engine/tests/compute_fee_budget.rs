//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The per-transaction WASM compute budget is bounded by the fees paid. A transaction may run up to
//! `FREE_COMPUTE_GRACE_POINTS` of compute on credit — enough to source its fee (withdraw, claim-burn,
//! AMM swap, …) before it calls `pay_fee` — and beyond that each WASM call's metering allowance is
//! capped to the points the fees paid so far can cover. These tests prove a transaction cannot
//! extract more than the grace of unpaid compute, that paying more raises the allowance, and that
//! fee-sourcing compute within the grace is allowed in the fee intent.

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine::fees::FeeTable;
use tari_engine_types::{commit_result::RejectReason, fees::FeeSource, limits::FREE_COMPUTE_GRACE_POINTS};
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{ComponentAddress, NonFungibleAddress, TemplateAddress};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const METERING_BENCH: &str = "tests/templates/metering_bench";

/// A call sized above the grace (and above `grace + the tests' fee payment`, so it still traps when
/// a tiny fee is paid) but well under `MAX_WASM_POINTS_PER_TRANSACTION`, so the per-transaction hard
/// cap is never the binding constraint — only the payment-funded allowance is.
const ABOVE_GRACE_POINTS: u64 = FREE_COMPUTE_GRACE_POINTS * 2;
/// A call sized below the grace, runnable on credit before any fee is paid.
const BELOW_GRACE_POINTS: u64 = FREE_COMPUTE_GRACE_POINTS / 2;

struct Harness {
    test: TemplateTest,
    bench: TemplateAddress,
    account: ComponentAddress,
    owner: NonFungibleAddress,
    key: RistrettoSecretKey,
    per_round: u64,
}

/// Builds a test where only WASM execution is priced (1 fee unit per metering point) so a
/// transaction's fee charge equals the points it consumed and a small payment isolates the compute
/// budget, then calibrates points-per-round so calls can be sized to a known multiple of the grace.
fn setup() -> Harness {
    let mut test = TemplateTest::new(CRATE_PATH, [METERING_BENCH]);
    let bench = test.get_template_address("MeteringBench");
    let (account, owner, key) = test.create_funded_account();

    let mut fee_table = FeeTable::zero_rated();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();

    let per_round = {
        let mut points = |rounds: u64| -> u64 {
            let tx = Transaction::builder_localnet()
                .pay_fee_from_component(account, 900_000_000u64)
                .call_function(bench, "bench_div_u64", args![rounds])
                .build_and_seal(&key);
            let result = test.execute_expect_success(tx, vec![owner.clone()]);
            result
                .finalize
                .fee_receipt
                .fee_breakdown()
                .iter()
                .find_map(|(s, a)| (*s == FeeSource::WasmExecution).then_some(*a))
                .expect("WasmExecution charge present")
        };
        (points(20_000) - points(10_000)) / 10_000
    };

    Harness {
        test,
        bench,
        account,
        owner,
        key,
        per_round,
    }
}

fn assert_insufficient_fees(reason: &RejectReason) {
    assert!(
        matches!(reason, RejectReason::ExecutionFailure(msg) if msg.contains("Insufficient fees")),
        "expected an insufficient-fees-for-compute failure, got {reason:?}",
    );
}

/// A transaction that pays only a tiny fee cannot run more than the grace of compute, even though the
/// call is well under the per-transaction hard cap and would otherwise succeed. The fee intent still
/// commits and the whole payment is collected — the validator is paid for the grace compute it ran,
/// and only the compute beyond the payment is the (bounded) absorbed loss.
#[test]
fn underpaid_compute_is_capped_at_grace() {
    const FEE_PAYMENT: u64 = 1_000_000;

    let Harness {
        mut test,
        bench,
        account,
        owner,
        key,
        per_round,
    } = setup();

    let rounds = ABOVE_GRACE_POINTS / per_round;
    let tx = Transaction::builder_localnet()
        .pay_fee_from_component(account, FEE_PAYMENT)
        .call_function(bench, "bench_div_u64", args![rounds])
        .build_and_seal(&key);

    let result = test.try_execute(tx, vec![owner]).unwrap();
    assert_insufficient_fees(result.expect_failure());

    assert!(
        result.finalize.is_fee_only(),
        "expected the fee intent to commit and the rest to be rejected (AcceptFeeRejectRest)",
    );
    let fee_receipt = &result.finalize.fee_receipt;
    assert_eq!(
        fee_receipt.total_fees_paid(),
        FEE_PAYMENT,
        "the entire fee payment should be collected",
    );
    assert_eq!(
        fee_receipt.total_refunded(),
        0,
        "nothing is refunded when compute exceeds the payment"
    );
    assert!(
        fee_receipt.unpaid_debt() > 0,
        "compute charged beyond the payment is the bounded, absorbed loss",
    );
}

/// Paying more fees raises the compute allowance: the same call that fails when underpaid commits
/// when the fee funds compute beyond the grace.
#[test]
fn paying_more_raises_the_compute_allowance() {
    let Harness {
        mut test,
        bench,
        account,
        owner,
        key,
        per_round,
    } = setup();

    let rounds = ABOVE_GRACE_POINTS / per_round;
    let tx = Transaction::builder_localnet()
        .pay_fee_from_component(account, 50_000_000u64)
        .call_function(bench, "bench_div_u64", args![rounds])
        .build_and_seal(&key);

    test.execute_expect_success(tx, vec![owner]);
}

/// The fee intent may spend real compute to source its fee before it pays, as long as it stays within
/// the grace.
#[test]
fn fee_intent_may_spend_compute_within_grace_before_paying() {
    let Harness {
        mut test,
        bench,
        account,
        owner,
        key,
        per_round,
    } = setup();

    let rounds = BELOW_GRACE_POINTS / per_round;
    let tx = Transaction::builder_localnet()
        .with_fee_instructions_builder(|builder| {
            builder
                .call_function(bench, "bench_div_u64", args![rounds])
                .pay_fee_from_component(account, 20_000_000u64)
        })
        .build_and_seal(&key);

    test.execute_expect_success(tx, vec![owner]);
}

/// A fee intent that tries to run more than the grace of compute before paying traps out-of-gas and
/// the whole transaction is rejected — bounding the free compute extractable by stuffing an unbounded
/// loop into the fee intent.
#[test]
fn fee_intent_cannot_exceed_grace_compute() {
    let Harness {
        mut test,
        bench,
        account,
        owner,
        key,
        per_round,
    } = setup();

    let rounds = ABOVE_GRACE_POINTS / per_round;
    let tx = Transaction::builder_localnet()
        .with_fee_instructions_builder(|builder| {
            builder
                .call_function(bench, "bench_div_u64", args![rounds])
                .pay_fee_from_component(account, 50_000_000u64)
        })
        .build_and_seal(&key);

    let reason = test.execute_expect_failure(tx, vec![owner]);
    assert_insufficient_fees(&reason);
}
