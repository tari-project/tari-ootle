//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Orchestration-parity cross-check: drive the **same shared builder/seal/encode path that
//! `ootle-rs` sits on** for the committed public-transfer vectors, and assert it produces the
//! **identical** bytes + transaction id as the core's `build_and_encode_public_transfer`.
//!
//! ## What this proves (and what it does NOT)
//!
//! `ootle-rs` has **no independent encoder**. Its `IAccount::public_transfer`
//! (`crates/wallet/ootle-rs/src/builtin_templates/account.rs`) compiles down to the **same**
//! `tari_ootle_transaction::TransactionBuilder` recipe, the **same** `UnsealedTransactionV1` seal,
//! and the **same** `tari_bor` encoder that the core calls. So a core-vs-`ootle-rs` byte match
//! validates **orchestration parity** — the instruction sequence, the fee-instruction placement, the
//! input set/ordering, and the bucket-label convention — **NOT** that two independent CBOR encoders
//! agree (they share `tari_bor`). The truly-independent encoder drift check is the TS/Python re-ports.
//! This cross-check still earns its place: it red-builds the moment the core's orchestration drifts
//! from what production emits.
//!
//! ## Which path this test drives, and why
//!
//! `ootle-rs`'s `public_transfer` is a *synchronous* builder method, but constructing an
//! `AccountInvokeBuilder` requires a `Provider` and its only encode/seal exit is the async `prepare`
//! (input resolution) + the random-nonce seal. Pulling `ootle-rs` in (it depends on `tokio`,
//! `async-trait`, `tari_indexer_client`) would also drag forbidden/heavy deps into this crate's
//! dev-tree. So this test drives the **shared `tari_ootle_transaction` builder directly**, replaying
//! `ootle-rs`'s exact `public_transfer` recipe verbatim, and seals it through the core's
//! **deterministic** seal seam (`ootle_sdk_core::tx::seal_public_transfer_deterministic`) with the
//! **same pinned nonces** the vector uses. The recipe below is a **hand-copied snapshot** of
//! `account.rs`, kept in lockstep with it **manually** — so this driver and the core's `builder.rs`
//! check the same recipe by two routes that must agree, rather than calling one shared helper.
//!
//! **Maintenance:** if `account.rs`'s `public_transfer` recipe changes, this copy must be updated by
//! hand or the cross-check silently goes stale (it would keep matching its own frozen snapshot).
//!
//! ## Why pinned nonces on both sides
//!
//! `transaction_id` chains the auth + seal signatures, and the stock seal samples a random Schnorr
//! nonce, so a random-nonce seal could never byte-match the committed vector. Both the core (when it
//! generated the fixture) and this cross-check use the same pinned nonce secrets via the same
//! deterministic seal, which is the only way the bytes line up.

// The `harness` module is shared with the `golden_vectors` test binary; this binary uses only a
// subset of it, so suppress dead-code warnings for the helpers exercised by the other binary.
#[allow(dead_code)]
mod harness;

use harness::{Fixture, OP_BUILD_AND_ENCODE_PUBLIC_TRANSFER, fixtures_dir};
use ootle_sdk_core::{
    tx::{bor_encode, seal_public_transfer_deterministic},
    types::intent::{PublicTransferIntent, TransferRecipient},
};
use tari_ootle_transaction::{TransactionBuilder, UnsignedTransaction, args};
use tari_template_lib_types::Amount;

/// `ootle-rs`'s per-transfer bucket-label convention, hand-copied verbatim from
/// `AccountInvokeBuilder::next_bucket_name` rather than borrowing the core's label helper, so a drift
/// in either side's convention surfaces as a byte mismatch.
fn ootle_rs_bucket_name(builder: &TransactionBuilder) -> String {
    format!("__AccountInvokeBuilder_{}", builder.next_workspace_id())
}

/// Re-derives `ootle-rs`'s `IAccount::public_transfer` recipe (`account.rs`) directly on the
/// shared `tari_ootle_transaction::TransactionBuilder`, building the **unsigned** transaction.
///
/// This is a hand-copied snapshot of the production recipe — NOT a call into the core's
/// `build_public_transfer_unsigned` — kept in lockstep with `account.rs` by hand. It validates
/// orchestration parity against that snapshot: any drift in instruction order / fee placement /
/// inputs / bucket label shows up as a byte mismatch. If `account.rs` changes, update this copy too.
fn build_unsigned_via_shared_builder(network: u8, intent: &PublicTransferIntent) -> UnsignedTransaction {
    let from_component = intent.from_account.to_internal().expect("valid from-account");
    let resource = intent.resource_address.to_internal().expect("valid resource");
    let amount: Amount = intent.amount.to_internal();
    let fee: Amount = intent.fee.to_internal();
    let recipient_pk = match &intent.recipient {
        TransferRecipient::PublicKey(pk) => pk.to_internal(),
        TransferRecipient::Account(_) => {
            panic!("cross-check vectors use a public-key recipient (account recipient is unsupported here)")
        },
    };

    // --- ootle-rs recipe, verbatim (account.rs). ---
    let mut builder = TransactionBuilder::new(network);

    // `pay_fee` → pay_fee_from_component on the from-component.
    builder = builder.pay_fee_from_component(from_component, fee);

    // `public_transfer`: bucket name taken from `next_bucket_name()` BEFORE the
    // withdraw is added (matches ootle-rs ordering: `let bucket_name = self.next_bucket_name();`).
    let bucket_name = ootle_rs_bucket_name(&builder);
    builder = builder
        .call_method(from_component, "withdraw", args![resource, amount])
        .put_last_instruction_output_on_workspace(&bucket_name)
        .create_account_with_bucket(recipient_pk, bucket_name);

    // The cross-check skips `prepare`'s async input resolution by supplying inputs explicitly, in the
    // same fixed order the intent carries them (the inputs IndexSet ordering is byte-significant).
    let inputs = intent.inputs_to_internal().expect("valid inputs");
    builder = builder.with_inputs(inputs);

    builder = builder
        .with_min_epoch(intent.min_epoch.map(tari_ootle_common_types::Epoch))
        .with_max_epoch(intent.max_epoch.map(tari_ootle_common_types::Epoch))
        .with_dry_run(intent.dry_run);

    builder.build_unsigned()
}

/// Loads a committed real public-transfer fixture by its relative path under `fixtures/`.
fn load_real_fixture(rel: &str) -> Fixture {
    let path = fixtures_dir().join(rel);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The orchestration-parity assertion for one committed vector: the shared-builder path (same pinned
/// nonces) must encode the **identical** bytes + transaction id as the fixture's generator-owned
/// `expected`.
fn assert_crosscheck(rel: &str) {
    let fixture = load_real_fixture(rel);
    assert_eq!(
        fixture.operation, OP_BUILD_AND_ENCODE_PUBLIC_TRANSFER,
        "cross-check only covers the public-transfer operation"
    );

    let input = &fixture.input;
    // Encode fixtures always carry these (the parse op leaves them absent — but it is filtered above).
    let network = input.network.expect("encode fixture has a network");
    let intent = input.intent.as_ref().expect("encode fixture has an intent");
    let keys = input.keys.as_ref().expect("encode fixture has keys");

    // 1. Build the unsigned tx via the shared builder, re-deriving ootle-rs's recipe.
    let unsigned = build_unsigned_via_shared_builder(network.as_byte(), intent);

    // 2. Seal it deterministically with the SAME pinned nonces the vector pins, via the core's deterministic seal seam
    //    — the same seal leaf `ootle-rs` resolves to, but with fixed nonces so the bytes can match a committed vector.
    let (transaction, id) =
        seal_public_transfer_deterministic(unsigned, &keys.to_core()).expect("deterministic seal succeeds");

    // 3. Verify the deterministic signatures are real (not merely stable), then BOR-encode.
    assert!(
        transaction.verify_all_signatures(),
        "cross-check `{rel}`: sealed signatures must verify"
    );
    let encoded = bor_encode(transaction).expect("BOR encode succeeds");
    let encoded_hex = hex::encode(&encoded);
    let id_hex = hex::encode(id.as_bytes());

    // 4. Byte-for-byte parity against the generator-owned expected (hex diff on mismatch).
    assert_eq!(
        encoded_hex, fixture.expected.encoded_transaction,
        "\n*** ORCHESTRATION-PARITY MISMATCH: encoded_transaction ***\n fixture: {}\nexpected (core): {}\n  actual \
         (shared builder): {}\n",
        fixture.name, fixture.expected.encoded_transaction, encoded_hex,
    );
    assert_eq!(
        id_hex, fixture.expected.transaction_id,
        "\n*** ORCHESTRATION-PARITY MISMATCH: transaction_id ***\n fixture: {}\nexpected (core): {}\n  actual (shared \
         builder): {}\n",
        fixture.name, fixture.expected.transaction_id, id_hex,
    );
}

/// `single_key_basic`: the shared-builder path matches the core byte-for-byte.
#[test]
fn crosscheck_single_key_basic() {
    assert_crosscheck("public_transfer/single_key_basic.json");
}

/// `large_amount` (> 2^53 µTari): the shared-builder path matches the core byte-for-byte, proving the
/// u64-safe amount survives both drivers identically.
#[test]
fn crosscheck_large_amount() {
    assert_crosscheck("public_transfer/large_amount.json");
}

/// Sanity guard: a deliberately *wrong* amount must NOT match the committed bytes — proving the
/// cross-check actually discriminates on the encoded content (it would catch real orchestration
/// drift) rather than trivially passing.
#[test]
fn crosscheck_detects_divergence() {
    let mut fixture = load_real_fixture("public_transfer/single_key_basic.json");
    // Perturb the amount; the shared-builder encode must now differ from the committed expected.
    fixture
        .input
        .intent
        .as_mut()
        .expect("encode fixture has an intent")
        .amount = ootle_sdk_core::types::numeric::BoundaryAmount::new(999_999);
    let network = fixture.input.network.expect("encode fixture has a network");
    let intent = fixture.input.intent.as_ref().expect("encode fixture has an intent");
    let keys = fixture.input.keys.as_ref().expect("encode fixture has keys");
    let unsigned = build_unsigned_via_shared_builder(network.as_byte(), intent);
    let (transaction, _id) = seal_public_transfer_deterministic(unsigned, &keys.to_core()).expect("seal succeeds");
    let encoded_hex = hex::encode(bor_encode(transaction).expect("encode"));
    assert_ne!(
        encoded_hex, fixture.expected.encoded_transaction,
        "a perturbed amount must NOT match the committed bytes — else the cross-check proves nothing"
    );
}
