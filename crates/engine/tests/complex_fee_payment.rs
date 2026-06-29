//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The grace ([`FREE_COMPUTE_GRACE_POINTS`]) must accommodate the most complex *legitimate* way to
//! source a fee: a transaction that holds no TARI and acquires it inside the fee intent by swapping
//! another resource through an AMM pool, then pays from the resulting bucket. All of that runs on
//! credit before `pay_fee`, so it must fit within the grace. This test exercises that worst-case
//! flow end-to-end and asserts it both succeeds and keeps a safety margin below the grace — it fails
//! if the grace is ever set too low for a realistic fee-payment swap.

use tari_engine::fees::FeeTable;
use tari_engine_types::{fees::FeeSource, limits::FREE_COMPUTE_GRACE_POINTS};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{Amount, ComponentAddress, ResourceAddress, constants::TARI_TOKEN};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

/// The grace must stay at least this many times above the measured worst-case fee-sourcing compute.
/// The grace constant itself is sized with a larger margin; this guards against it drifting too low.
const SAFETY_FACTOR: u64 = 2;

const POOL_LIQUIDITY: u64 = 100_000_000;
const SWAP_INPUT: u64 = 20_000_000;

fn create_faucet(test: &mut TemplateTest, symbol: &str) -> (ComponentAddress, ResourceAddress) {
    let component: ComponentAddress = test.call_function(
        "TestFaucet",
        "mint_with_symbol",
        args![Amount::from(1_000_000_000_000u64), symbol.to_string()],
        vec![],
    );
    let resource = test
        .get_previous_output_address(SubstateType::Resource)
        .as_resource_address()
        .unwrap();
    (component, resource)
}

fn create_pool(test: &mut TemplateTest, a: ResourceAddress, b: ResourceAddress) -> ComponentAddress {
    let template = test.get_template_address("TariSwapPool");
    let result = test.execute_expect_success(
        test.transaction()
            .call_function(template, "new", args![a, b, 5u16])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    let (addr, _) = result
        .expect_success()
        .up_iter()
        .find(|(address, substate)| {
            address.is_component() && *substate.substate_value().component().unwrap().template_address() == template
        })
        .unwrap();
    addr.as_component_address().unwrap()
}

#[test]
fn fee_paid_via_amm_swap_fits_within_grace() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/tariswap", "tests/templates/faucet"]);
    let (account, owner, key) = test.create_funded_account();

    // A non-TARI resource the account holds and will swap for TARI to pay its fee.
    let (token_faucet, token) = create_faucet(&mut test, "SWAP");
    test.build_and_execute(
        Transaction::builder_localnet()
            .call_method(token_faucet, "take_free_coins_custom", args![Amount::from(
                1_000_000_000u64
            )])
            .put_last_instruction_output_on_workspace("coins")
            .call_method(account, "deposit", args![Workspace("coins")]),
        vec![],
    )
    .expect_success();

    // A (TARI, token) pool, seeded with liquidity from the funded account.
    let pool = create_pool(&mut test, TARI_TOKEN, token);
    test.build_and_execute(
        Transaction::builder_localnet()
            .call_method(account, "withdraw", args![TARI_TOKEN, Amount::from(POOL_LIQUIDITY)])
            .put_last_instruction_output_on_workspace("tari")
            .call_method(account, "withdraw", args![token, Amount::from(POOL_LIQUIDITY)])
            .put_last_instruction_output_on_workspace("token")
            .call_method(pool, "add_liquidity", args![Workspace("tari"), Workspace("token")])
            .put_last_instruction_output_on_workspace("lp")
            .call_method(account, "deposit", args![Workspace("lp")]),
        vec![owner.clone(), test.owner_proof()],
    )
    .expect_success();

    // Price WASM execution at 1 fee unit per metering point so the WasmExecution charge equals the
    // points consumed, then enable fees so the swap-to-pay-fee flow is metered under the real grace.
    let mut fee_table = FeeTable::zero_rated();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();

    // The worst-case fee payment: source TARI by swapping `token` through the pool inside the fee
    // intent, then pay from the resulting bucket. Runs entirely on credit before `pay_fee`.
    let tx = Transaction::builder_localnet()
        .with_fee_instructions_builder(|builder| {
            builder
                .call_method(account, "withdraw", args![token, Amount::from(SWAP_INPUT)])
                .put_last_instruction_output_on_workspace("input")
                .call_method(pool, "swap", args![Workspace("input"), TARI_TOKEN])
                .put_last_instruction_output_on_workspace("fee_tari")
                .pay_fee_from_bucket("fee_tari")
        })
        .build_and_seal(&key);

    let result = test.execute_expect_success(tx, vec![owner]);

    let worst_case_points = result
        .finalize
        .fee_receipt
        .fee_breakdown()
        .iter()
        .find_map(|(s, a)| (*s == FeeSource::WasmExecution).then_some(*a))
        .expect("WasmExecution charge present");

    eprintln!(
        "worst-case fee-sourcing compute (AMM swap to TARI): {worst_case_points} points; FREE_COMPUTE_GRACE_POINTS = \
         {FREE_COMPUTE_GRACE_POINTS}",
    );
    assert!(
        worst_case_points.saturating_mul(SAFETY_FACTOR) <= FREE_COMPUTE_GRACE_POINTS,
        "FREE_COMPUTE_GRACE_POINTS ({FREE_COMPUTE_GRACE_POINTS}) must stay at least {SAFETY_FACTOR}x above the \
         worst-case fee-sourcing compute ({worst_case_points}); re-measure and adjust the grace constant",
    );
}
