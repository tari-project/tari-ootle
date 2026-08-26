//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Groth16/BN254 verification performed entirely inside a WASM template, with no engine support
//! beyond the metering budget. These tests fix what that costs, so the budget
//! (`limits::MAX_WASM_POINTS_PER_TRANSACTION`) and the metering table cannot drift apart from it
//! silently: a re-costing or a toolchain change that makes an in-WASM verifier stop fitting is a
//! deliberate decision, not something to discover from a rejected transaction.
//!
//! `examples/zk_points_calibrate.rs` prints the full cost breakdown these bounds come from.

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{
    commit_result::RejectReason,
    limits::{FREE_COMPUTE_GRACE_POINTS, MAX_WASM_POINTS_PER_TRANSACTION},
};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_template_lib::types::{ComponentAddress, NonFungibleAddress, TemplateAddress, bytes::Bytes};
use tari_template_test_tooling::TemplateTest;

#[path = "support/groth16.rs"]
mod groth16;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const ZK_VERIFIER: &str = "tests/templates/zk_verifier";

/// Public input count the cost bounds are stated at: a realistic contract statement (a root, a
/// nullifier, a recipient, an amount).
const INPUTS: usize = 4;

/// Points a verify at [`INPUTS`] must stay under. The measured cost is ~112M; the headroom absorbs
/// toolchain and arkworks drift without letting a regression that eats the budget go unnoticed.
const MAX_EXPECTED_POINTS: u64 = 160_000_000;

/// Points a verify at [`INPUTS`] must exceed. Guards against the assertions passing on a template
/// that silently stopped doing the work — a verify is pairing-heavy and cannot be cheap.
const MIN_EXPECTED_POINTS: u64 = 40_000_000;

/// Points a verify must leave unspent for the contract logic around it. A budget that a
/// verification alone exhausts is not a budget a contract can verify *in*, so this is the property
/// the per-transaction ceiling was sized on and the one that has to keep holding.
const MIN_CONTRACT_HEADROOM_POINTS: u64 = 60_000_000;

struct Harness {
    test: TemplateTest,
    template: TemplateAddress,
    account: ComponentAddress,
    owner: NonFungibleAddress,
    key: RistrettoSecretKey,
}

fn setup() -> Harness {
    let mut test = TemplateTest::new(CRATE_PATH, [ZK_VERIFIER]);
    let template = test.get_template_address("ZkVerifier");
    let (account, owner, key) = test.create_funded_account();
    Harness {
        test,
        template,
        account,
        owner,
        key,
    }
}

impl Harness {
    /// Calls `verify_prepared` once and returns whether the proof verified, along with the WASM
    /// metering points the transaction consumed. Fees stay disabled so the call gets the full
    /// per-transaction budget rather than an allowance bounded by what it has paid — what is under
    /// test is the hard cap, not the fee model.
    fn verify(&mut self, f: &groth16::Fixture, inputs: &Bytes) -> (bool, u64) {
        let tx = self
            .test
            .transaction()
            .call_function(self.template, "verify_prepared", args![
                f.pvk_uncompressed.clone(),
                f.proof_uncompressed.clone(),
                inputs.clone()
            ])
            .build_and_seal(&self.key);
        let result = self.test.execute_expect_success(tx, vec![]);
        let verified = result.finalize.execution_results[0].decode::<bool>().unwrap();
        (verified, result.wasm_execution_points)
    }
}

/// A Groth16 proof verifies inside a template, and the whole thing fits the per-transaction budget
/// with room left for the contract logic that would surround it.
#[test]
fn groth16_verify_fits_the_budget() {
    let mut h = setup();
    let f = groth16::fixture(INPUTS);

    let (verified, points) = h.verify(&f, &f.inputs);

    assert!(verified, "a valid proof must verify");
    assert!(
        points < MAX_EXPECTED_POINTS,
        "in-WASM Groth16 verify cost {points} points, above the {MAX_EXPECTED_POINTS} bound",
    );
    assert!(
        points > MIN_EXPECTED_POINTS,
        "in-WASM Groth16 verify cost only {points} points — too cheap to be doing the work",
    );
    let headroom = MAX_WASM_POINTS_PER_TRANSACTION.saturating_sub(points);
    assert!(
        headroom >= MIN_CONTRACT_HEADROOM_POINTS,
        "a verify took {points} of {MAX_WASM_POINTS_PER_TRANSACTION} points, leaving only {headroom} for the contract \
         around it",
    );
}

/// The flow a contract actually uses: the verifying key lives in component state and a caller
/// supplies only a proof and the statement it attests to. Distinct from the static paths above,
/// which take the key as an argument — here the key is trusted state the caller cannot substitute.
#[test]
fn groth16_verifies_against_a_component_held_key() {
    let mut h = setup();
    let f = groth16::fixture(INPUTS);

    let create = h
        .test
        .transaction()
        .call_function(h.template, "new", args![f.pvk_uncompressed.clone()])
        .build_and_seal(&h.key);
    h.test.execute_expect_success(create, vec![]);
    let component = h
        .test
        .get_previous_output_address(SubstateType::Component)
        .as_component_address()
        .unwrap();

    let verified = h.test.call_method::<bool>(
        component,
        "verify_stateful",
        args![f.proof_uncompressed.clone(), f.inputs.clone()],
        vec![],
    );
    assert!(verified, "a valid proof must verify against the component's key");

    // Loading the key from state rather than taking it as an argument must not change what the
    // verification costs — the same decode and the same pairing check, off the same bytes.
    let points = h.test.last_execution_points().total();
    assert!(
        (MIN_EXPECTED_POINTS..MAX_EXPECTED_POINTS).contains(&points),
        "verifying against a component-held key cost {points} points, outside the \
         {MIN_EXPECTED_POINTS}..{MAX_EXPECTED_POINTS} band the static paths sit in",
    );

    let verified = h.test.call_method::<bool>(
        component,
        "verify_stateful",
        args![f.proof_uncompressed.clone(), f.wrong_inputs.clone()],
        vec![],
    );
    assert!(
        !verified,
        "the component's key must not verify a statement the proof does not attest to"
    );
}

/// A caller may supply the verifying key itself, in either encoding. Both accept a valid proof and
/// both fit the budget — compression trades bytes on the wire for point decompression, and
/// supplying an unprepared key trades a stored 34 KiB prepared key for a pairing per call.
#[test]
fn groth16_verifies_from_a_caller_supplied_key() {
    let mut h = setup();
    let f = groth16::fixture(INPUTS);

    for (func, vk, proof) in [
        ("verify_uncompressed", &f.vk_uncompressed, &f.proof_uncompressed),
        ("verify_compressed", &f.vk_compressed, &f.proof_compressed),
    ] {
        let tx = h
            .test
            .transaction()
            .call_function(h.template, func, args![vk.clone(), proof.clone(), f.inputs.clone()])
            .build_and_seal(&h.key);
        let result = h.test.execute_expect_success(tx, vec![]);

        assert!(
            result.finalize.execution_results[0].decode::<bool>().unwrap(),
            "{func} must verify a valid proof",
        );
        assert!(
            result.wasm_execution_points < MAX_WASM_POINTS_PER_TRANSACTION,
            "{func} cost {} points, over the {MAX_WASM_POINTS_PER_TRANSACTION} budget",
            result.wasm_execution_points,
        );
    }
}

/// Public inputs the proof does not attest to are rejected, and rejection is not cheaper than
/// acceptance — both run the same pairing check, so a caller learns nothing from the cost.
#[test]
fn groth16_rejects_wrong_public_inputs() {
    let mut h = setup();
    let f = groth16::fixture(INPUTS);

    let (verified, rejected_points) = h.verify(&f, &f.wrong_inputs);
    assert!(!verified, "public inputs the proof does not attest to must not verify");

    let (_, accepted_points) = h.verify(&f, &f.inputs);
    let ratio = rejected_points as f64 / accepted_points as f64;
    assert!(
        (0.8..1.2).contains(&ratio),
        "rejecting cost {rejected_points} points against {accepted_points} to accept",
    );
}

/// Verification cost grows with the public input count — the `gamma_abc_g1` MSM is the only
/// input-dependent work — and stays inside the budget across the range a contract would plausibly
/// use. Sixteen is the top of that range: cost is linear in the count at ~5.3M points each, so a
/// statement much wider than this belongs behind a hash, as circuits conventionally put it.
#[test]
fn groth16_verify_scales_with_public_inputs() {
    let mut h = setup();

    let mut measured = Vec::new();
    for n in [1usize, 4, 16] {
        let f = groth16::fixture(n);
        let (verified, points) = h.verify(&f, &f.inputs);
        assert!(verified, "a valid proof with {n} public inputs must verify");
        assert!(
            points < MAX_WASM_POINTS_PER_TRANSACTION,
            "{n} public inputs cost {points} points, over the {MAX_WASM_POINTS_PER_TRANSACTION} budget",
        );
        measured.push((n, points));
    }

    for pair in measured.windows(2) {
        let [(lo_n, lo), (hi_n, hi)] = pair else { unreachable!() };
        assert!(
            hi > lo,
            "{hi_n} public inputs cost {hi} points, not more than {lo} at {lo_n}",
        );
    }
}

/// Stacking verifies exhausts the transaction-wide budget rather than getting a fresh per-call one.
/// Without this a contract could verify unboundedly many proofs in a single transaction by putting
/// each in its own instruction.
#[test]
fn stacked_verifies_exhaust_the_transaction_budget() {
    let mut h = setup();
    let f = groth16::fixture(INPUTS);

    // Sized off the bound rather than the measured cost, so the count is guaranteed to overrun the
    // budget however the real per-verify cost drifts within it.
    let count = (MAX_WASM_POINTS_PER_TRANSACTION / MIN_EXPECTED_POINTS + 1) as usize;
    let tx = h
        .test
        .transaction()
        .fold(0..count, |builder, _| {
            builder.call_function(h.template, "verify_prepared", args![
                f.pvk_uncompressed.clone(),
                f.proof_uncompressed.clone(),
                f.inputs.clone()
            ])
        })
        .build_and_seal(&h.key);

    let reason = h.test.execute_expect_failure(tx, vec![]);
    assert!(
        matches!(reason, RejectReason::ExecutionFailure(_)),
        "expected {count} stacked verifies to run out of gas, got {reason:?}",
    );
}

/// A verify cannot be funded by the fee intent's flat compute credit. The credit exists to let a
/// transaction source its fee before paying; anything this expensive belongs in the main
/// instructions, where the fee already paid funds it.
#[test]
fn verify_does_not_fit_the_fee_intent_credit() {
    const _: () = assert!(FREE_COMPUTE_GRACE_POINTS < MIN_EXPECTED_POINTS);

    let Harness {
        mut test,
        template,
        account,
        owner,
        key,
    } = setup();
    let f = groth16::fixture(INPUTS);

    test.enable_fees();
    let tx = Transaction::builder_localnet(Epoch(1))
        .with_fee_instructions_builder(|builder| {
            builder
                .call_function(template, "verify_prepared", args![
                    f.pvk_uncompressed.clone(),
                    f.proof_uncompressed.clone(),
                    f.inputs.clone()
                ])
                .pay_fee_from_component(account, 500_000_000u64)
        })
        .build_and_seal(&key);

    let reason = test.execute_expect_failure(tx, vec![owner]);
    assert!(
        matches!(&reason, RejectReason::ExecutionFailure(msg) if msg.contains("compute credit")),
        "expected the fee intent's compute credit to be the binding limit, got {reason:?}",
    );
}
