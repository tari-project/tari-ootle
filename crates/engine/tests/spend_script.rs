//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Engine tests for TIP-0006 spend-time scripts (`SpendCondition::Script`).

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_common_types::{
    RistrettoSchnorrBlake2bVerifier,
    crypto::create_key_pair_from_seed,
    substate_type::SubstateType,
};
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{
    FunctionName,
    ResourceAddress,
    TemplateAddress,
    bytes::Bytes,
    constants::STEALTH_TARI_RESOURCE_ADDRESS,
    crypto::{NoSignatureDomain, PublicKey, RistrettoPublicKeyBytes, Signature},
    stealth::{SpendCondition, SpendScript},
};
use tari_template_test_tooling::{
    TemplateTest,
    support::{
        assert_error::assert_reject_reason,
        stealth,
        stealth::{NO_INPUTS, StealthSecretTransferData},
    },
    wallet_crypto::MaskAndValue,
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const TEMPLATE_PATHS: &[&str] = &["tests/templates/stealth", "tests/templates/spend_script"];
const FAUCET_TEMPLATE: &str = "StealthFaucet";
const SCRIPT_TEMPLATE: &str = "SpendScripts";

fn spend_script(template: TemplateAddress, function: &str, args: Vec<Bytes>) -> SpendScript {
    SpendScript::new(template, FunctionName::try_from(function).unwrap(), args)
}

fn script_condition(template: TemplateAddress, function: &str, args: Vec<Bytes>) -> SpendCondition {
    SpendCondition::Script(spend_script(template, function, args))
}

fn encode_arg<T: tari_bor::Encode<()>>(value: &T) -> Bytes {
    Bytes::from_vec(tari_bor::encode(value).unwrap())
}

/// Builds the (sealed) `StealthFaucet::new` transaction which mints `mint`'s outputs. Used both for the happy path
/// (expect success) and for creation-time (T1) rejection tests (expect failure).
fn faucet_new_tx(test: &mut TemplateTest, mint: &StealthSecretTransferData) -> Transaction {
    test.enable_auto_add_proofs_from_signers();
    let faucet_template = test.get_template_address(FAUCET_TEMPLATE);
    let initial_supply = mint.statement.inputs_statement.revealed_amount;
    Transaction::builder_localnet()
        .call_function(faucet_template, "new", args![
            initial_supply,
            mint.statement.clone(),
            None::<RistrettoPublicKeyBytes>
        ])
        .build_and_seal(test.secret_key())
}

/// Mints a single 100-unit stealth UTXO gated by `condition` and returns the resource address plus the mint secrets
/// (the output mask is needed to spend the UTXO).
fn mint_utxo(test: &mut TemplateTest, condition: SpendCondition) -> (ResourceAddress, StealthSecretTransferData) {
    let mint = stealth::generate_mint_statement(vec![(100u64, condition)], 0u64, None);
    let tx = faucet_new_tx(test, &mint);
    test.execute_expect_success(tx, vec![]);
    let resx = test
        .get_previous_output_address(SubstateType::Resource)
        .as_resource_address()
        .unwrap();
    (resx, mint)
}

/// Builds a transfer that spends the single minted UTXO into the provided output condition.
fn spend_into(mint: &StealthSecretTransferData, output: SpendCondition) -> StealthSecretTransferData {
    stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0u64,
        [(100u64, output)],
        0u64,
    )
}

// -------------------------------- Timelock -------------------------------- //

#[test]
fn timelock_allows_spend_at_or_after_unlock_epoch() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (resx, mint) = mint_utxo(
        &mut test,
        script_condition(script_template, "timelock", vec![encode_arg(&0u64)]),
    );

    // Output is signed (the default); the timelock is on the input being spent.
    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn timelock_rejects_spend_before_unlock_epoch() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (resx, mint) = mint_utxo(
        &mut test,
        script_condition(script_template, "timelock", vec![encode_arg(&u64::MAX)]),
    );

    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
    assert_reject_reason(&reason, "timelock");
}

// -------------------------------- Recursive covenant -------------------------------- //

#[test]
fn covenant_allows_output_that_preserves_condition() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let covenant = script_condition(script_template, "preserve_covenant", vec![]);
    let (resx, mint) = mint_utxo(&mut test, covenant.clone());

    // The output carries the same covenant condition -> the covenant is satisfied.
    let transfer = spend_into(&mint, covenant);
    test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn covenant_rejects_output_that_changes_condition() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let covenant = script_condition(script_template, "preserve_covenant", vec![]);
    let (resx, mint) = mint_utxo(&mut test, covenant);

    // The output changes the condition to Signed -> the covenant rejects the spend.
    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
}

#[test]
fn covenant_rejects_spend_with_no_stealth_outputs() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let covenant = script_condition(script_template, "preserve_covenant", vec![]);
    let (resx, mint) = mint_utxo(&mut test, covenant);

    // Reveal the whole 100 units, producing zero stealth outputs. The covenant requires at least one output that
    // preserves the condition, so the spend is rejected.
    let transfer = stealth::generate_transfer_data(
        [MaskAndValue {
            mask: mint.output_masks[0].clone(),
            value: 100,
        }],
        0u64,
        std::iter::empty::<(u64, SpendCondition)>(),
        100u64,
    );
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
}

// -------------------------------- Unconditional reject -------------------------------- //

#[test]
fn always_reject_aborts_spend() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (resx, mint) = mint_utxo(&mut test, script_condition(script_template, "always_reject", vec![]));

    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
}

// -------------------------------- Read-only sandbox -------------------------------- //

#[test]
fn read_only_sandbox_blocks_state_mutation() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (resx, mint) = mint_utxo(&mut test, script_condition(script_template, "try_write", vec![]));

    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
    assert_reject_reason(&reason, "read-only execution context");
}

#[test]
fn sandbox_denies_emit_event() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (resx, mint) = mint_utxo(&mut test, script_condition(script_template, "try_emit_event", vec![]));

    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
    assert_reject_reason(&reason, "forbidden inside a read-only");
}

// -------------------------------- Creation-time (T1) validation -------------------------------- //

/// Asserts that minting an output carrying `bad` is rejected at creation time with a reason containing `expected`.
fn assert_creation_rejected(function: &str, args: Vec<Bytes>, expected: &str) {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let mint = stealth::generate_mint_statement(
        vec![(100u64, script_condition(script_template, function, args))],
        0u64,
        None,
    );
    let tx = faucet_new_tx(&mut test, &mint);
    let reason = test.execute_expect_failure(tx, vec![]);
    assert_reject_reason(&reason, expected);
}

#[test]
fn creation_rejects_unknown_function() {
    assert_creation_rejected("does_not_exist", vec![], "not found");
}

#[test]
fn creation_rejects_mutable_function() {
    assert_creation_rejected("bad_mutable", vec![], "must not be mutable");
}

#[test]
fn creation_rejects_non_unit_function() {
    assert_creation_rejected("bad_returns_value", vec![], "must return unit");
}

#[test]
fn creation_rejects_missing_context_arg() {
    assert_creation_rejected("bad_no_context", vec![encode_arg(&0u64)], "must take a SpendContext");
}

#[test]
fn creation_rejects_wrong_bound_arg_count() {
    // `timelock` expects exactly one bound argument; provide none.
    assert_creation_rejected("timelock", vec![], "bound argument");
}

#[test]
fn creation_rejects_malformed_bound_arg() {
    // `timelock` expects one bound argument; provide one whose bytes are not well-formed CBOR (trailing data).
    assert_creation_rejected("timelock", vec![Bytes::from_vec(vec![0x00, 0x00])], "well-formed CBOR");
}

// -------------------------------- Signature lock -------------------------------- //

// Must match `SpendSigDomain` / `SIG_MESSAGE` in the spend_script template.
const SPEND_SIG_DOMAIN: &[u8] = b"tari.test.spend_script signature domain";
const SPEND_SIG_MESSAGE: &[u8] = b"spend authorisation";

fn sign_spend(secret: &RistrettoSecretKey, message: &[u8]) -> Signature<NoSignatureDomain> {
    let (nonce, nonce_pub) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let public_key = RistrettoPublicKey::from_secret_key(secret);
    let challenge = RistrettoSchnorrBlake2bVerifier::compute_challenge(
        SPEND_SIG_DOMAIN,
        message,
        &public_key.to_byte_type(),
        &nonce_pub.to_byte_type(),
    );
    let sig = RistrettoSchnorr::sign_raw_uniform(secret, nonce, &challenge).unwrap();
    sig.to_byte_type().into()
}

#[test]
fn signature_lock_allows_valid_signature() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (secret, public) = create_key_pair_from_seed(7);
    let public = PublicKey::from(public.to_byte_type());
    let signature = sign_spend(&secret, SPEND_SIG_MESSAGE);

    let condition = script_condition(script_template, "require_signature", vec![
        encode_arg(&public),
        encode_arg(&signature),
    ]);
    let (resx, mint) = mint_utxo(&mut test, condition);

    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    test.execute_expect_success(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
}

#[test]
fn signature_lock_rejects_invalid_signature() {
    let mut test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);
    let (secret, public) = create_key_pair_from_seed(7);
    let public = PublicKey::from(public.to_byte_type());
    // Sign a different message -> the bound signature is invalid for SPEND_SIG_MESSAGE.
    let signature = sign_spend(&secret, b"some other message");

    let condition = script_condition(script_template, "require_signature", vec![
        encode_arg(&public),
        encode_arg(&signature),
    ]);
    let (resx, mint) = mint_utxo(&mut test, condition);

    let transfer = spend_into(&mint, SpendCondition::Signed(test.to_public_key_bytes()));
    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .stealth_transfer(resx, transfer.statement)
            .finish()
            .seal(test.secret_key()),
        vec![],
    );
    assert_reject_reason(&reason, "Spend script rejected the spend");
    assert_reject_reason(&reason, "invalid spend signature");
}

// -------------------------------- Weight -------------------------------- //

#[test]
fn script_output_increases_transaction_weight() {
    let test = TemplateTest::new(CRATE_PATH, TEMPLATE_PATHS);
    let script_template = test.get_template_address(SCRIPT_TEMPLATE);

    // A large-`args` Script condition vs an equivalent Signed condition on the same output. The weight is computed
    // statically (no execution), so the script need not be well-formed for this comparison.
    let large_script = script_condition(script_template, "always_ok", vec![Bytes::from_vec(vec![7u8; 2000])]);
    let script_transfer = stealth::generate_transfer_data(NO_INPUTS, 100u64, [(100u64, large_script)], 0u64);
    let signed_transfer = stealth::generate_transfer_data(
        NO_INPUTS,
        100u64,
        [(100u64, SpendCondition::Signed(test.to_public_key_bytes()))],
        0u64,
    );

    let script_tx = Transaction::builder_localnet()
        .stealth_transfer(STEALTH_TARI_RESOURCE_ADDRESS, script_transfer.statement)
        .finish()
        .seal(test.secret_key());
    let signed_tx = Transaction::builder_localnet()
        .stealth_transfer(STEALTH_TARI_RESOURCE_ADDRESS, signed_transfer.statement)
        .finish()
        .seal(test.secret_key());

    assert!(
        script_tx.calculate_transaction_weight().as_u64() > signed_tx.calculate_transaction_weight().as_u64(),
        "Script-conditioned output should weigh more than a Signed one",
    );
}
