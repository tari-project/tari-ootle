//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Native verification (stealth transfers, confidential withdraws, burn claims) is priced in
//! WASM-point equivalents and charged against the same payment-funded allowance as WASM execution,
//! *before* the crypto runs. These tests prove the three load-bearing properties:
//!
//! 1. the grace covers every legitimate fee-sourcing flow (sizing relation over the constants);
//! 2. a transaction that does not pay is rejected before performing native verification, so garbage proofs cannot
//!    extract crypto work from a validator;
//! 3. a paying transaction is charged for its native verification under `FeeSource::NativeExecution`.

use ootle_byte_type::ToByteType;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine::fees::FeeTable;
use tari_engine_types::{
    fees::FeeSource,
    limits::{FREE_COMPUTE_GRACE_POINTS, NativeExecutionPoints},
};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{ComponentAddress, NonFungibleAddress, ResourceAddress};
use tari_template_test_tooling::{
    TemplateTest,
    support::{assert_error::assert_reject_reason, stealth, stealth::StealthSecretTransferData},
    wallet_crypto::MaskAndValue,
};

const TEMPLATE_PATHS: &[&str] = &["tests/templates/stealth"];
const TEMPLATE_NAME: &str = "StealthFaucet";
const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

/// The grace must cover every legitimate fee-sourcing flow with margin, since all of it runs on
/// credit before `pay_fee`. The stealth-UTXO-funded fee is the ceiling: one transfer statement
/// with a single stealth change output and up to 64 dust inputs. The AMM-swap (WASM) flow is
/// guarded end-to-end by `complex_fee_payment`.
#[test]
#[allow(clippy::assertions_on_constants)]
fn grace_covers_legitimate_fee_sourcing_flows() {
    const SAFETY_FACTOR: u64 = 2;
    const FEE_SOURCE_DUST_INPUTS: u64 = 64;

    let stealth_fee_source = NativeExecutionPoints::PER_STATEMENT +
        NativeExecutionPoints::PER_OUTPUT +
        FEE_SOURCE_DUST_INPUTS * NativeExecutionPoints::PER_INPUT;
    assert!(
        stealth_fee_source * SAFETY_FACTOR <= FREE_COMPUTE_GRACE_POINTS,
        "FREE_COMPUTE_GRACE_POINTS ({FREE_COMPUTE_GRACE_POINTS}) must stay at least {SAFETY_FACTOR}x above the \
         stealth fee-sourcing flow ({stealth_fee_source} points)",
    );

    assert!(
        NativeExecutionPoints::PER_CLAIM_BURN * SAFETY_FACTOR <= FREE_COMPUTE_GRACE_POINTS,
        "FREE_COMPUTE_GRACE_POINTS ({FREE_COMPUTE_GRACE_POINTS}) must stay at least {SAFETY_FACTOR}x above a burn \
         claim ({} points)",
        NativeExecutionPoints::PER_CLAIM_BURN,
    );
}

fn setup_faucet(
    test: &mut TemplateTest,
    transfer_data: &StealthSecretTransferData,
) -> (ComponentAddress, ResourceAddress) {
    test.enable_auto_add_proofs_from_signers();
    let template_addr = test.get_template_address(TEMPLATE_NAME);
    let initial_supply = transfer_data.statement.inputs_statement.revealed_amount;

    let transaction = Transaction::builder_localnet()
        .call_function(template_addr, "new", args![
            initial_supply,
            transfer_data.statement,
            Option::<tari_template_lib::types::crypto::RistrettoPublicKeyBytes>::None
        ])
        .build_and_seal(test.secret_key());

    test.execute_expect_success(transaction, vec![]);

    let faucet = test.get_previous_output_address(SubstateType::Component);
    let resx = test.get_previous_output_address(SubstateType::Resource);

    (
        faucet.as_component_address().unwrap(),
        resx.as_resource_address().unwrap(),
    )
}

fn enable_point_priced_fees(test: &mut TemplateTest) {
    // 1 fee unit per metering point so charges equal points and small payments isolate the budget.
    let mut fee_table = FeeTable::zero_rated();
    fee_table.per_wasm_point_cost = 1;
    fee_table.wasm_points_cost_divisor = 1;
    test.set_fee_table(fee_table);
    test.enable_fees();
}

/// A non-paying transaction whose fee intent carries a statement priced above the grace is rejected
/// by the allowance pre-charge — before any of its crypto runs. The statement's range proof is
/// deliberately corrupted: were the verification performed, the failure would be a range-proof
/// error, so the insufficient-fees rejection proves the charge fired first.
#[test]
fn unpaid_native_verification_traps_before_the_crypto_runs() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let mint = stealth::generate_mint_statement([100, 1000], 0u64, None);
    let (_faucet, faucet_resx) = setup_faucet(&mut test, &mint);
    enable_point_priced_fees(&mut test);

    // 8 outputs price above the 32M grace (fixed + 8 × per-output ≈ 66M points).
    let mut garbage = stealth::generate_mint_statement(vec![100u64; 8], 0u64, None);
    let mut rp = garbage.statement.outputs_statement.agg_range_proof.clone().into_vec();
    rp[100] ^= 0xFF;
    garbage.statement.outputs_statement.agg_range_proof = rp.try_into().unwrap();

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .with_fee_instructions_builder(|builder| builder.stealth_transfer(faucet_resx, garbage.statement))
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, "Insufficient fees to fund native verification");
}

/// A paying transaction's native verification is charged under `FeeSource::NativeExecution` at the
/// per-point rate.
#[test]
fn paid_native_verification_is_charged() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let mint = stealth::generate_mint_statement([100, 1000], 0u64, None);
    let (_faucet, faucet_resx) = setup_faucet(&mut test, &mint);
    let (account, owner, key) = test.create_funded_account();
    enable_point_priced_fees(&mut test);

    let transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0u64,
        Some(100),
        0,
    );
    let expected_points = tari_engine_types::stealth::transfer_native_points(&transfer.statement);

    let seal_signer = RistrettoPublicKey::from_secret_key(&key).to_byte_type();
    // Explicit proofs suppress the tooling's auto-added signer badges, so the UTXO spend key's
    // badge must be supplied alongside the fee account's owner badge.
    let mask_badge =
        NonFungibleAddress::from_public_key(RistrettoPublicKey::from_secret_key(&mint.output_masks[0]).to_byte_type());
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(account, 900_000_000u64)
            .stealth_transfer(faucet_resx, transfer.statement)
            .finish()
            .add_signer(&seal_signer, &mint.output_masks[0])
            .seal(&key),
        vec![owner, mask_badge],
    );

    let native_charge = result
        .finalize
        .fee_receipt
        .fee_breakdown()
        .iter()
        .find_map(|(s, a)| (*s == FeeSource::NativeExecution).then_some(*a))
        .expect("NativeExecution charge present");
    assert_eq!(native_charge, expected_points);
}
