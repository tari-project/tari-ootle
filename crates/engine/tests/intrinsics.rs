//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Checks each intrinsic against the same computation performed natively on the host, and checks the
//! algebraic identities that only hold when several of them agree.
//!
//! A per-primitive equality test catches a wrong implementation; it does not catch a primitive wired
//! to the wrong operation, because both sides would move together. The identity tests
//! (`the_group_is_homomorphic_over_scalar_addition`, `commitments_add`) are what close that gap.

use digest::Digest;
use tari_crypto::{
    keys::{PublicKey as _, SecretKey as _},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_engine_types::fees::FeeSource;
use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_template_lib::types::{
    Hash32,
    Hash64,
    crypto::{RistrettoPublicKeyBytes, Scalar32Bytes},
};
use tari_template_test_tooling::TemplateTest;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const INTRINSICS: &str = "tests/templates/intrinsics";
const TEMPLATE: &str = "IntrinsicsTest";

fn setup() -> TemplateTest {
    TemplateTest::new(CRATE_PATH, [INTRINSICS])
}

/// Deterministic scalars, so a failure reproduces exactly.
fn scalar(seed: u64) -> RistrettoSecretKey {
    let mut bytes = [0u8; 64];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes[8] = 1;
    RistrettoSecretKey::from_uniform_bytes(&bytes).unwrap()
}

fn point(seed: u64) -> RistrettoPublicKey {
    RistrettoPublicKey::from_secret_key(&scalar(seed))
}

fn pk_bytes(p: &RistrettoPublicKey) -> RistrettoPublicKeyBytes {
    RistrettoPublicKeyBytes::from_bytes(p.as_bytes()).unwrap()
}

fn sk_bytes(s: &RistrettoSecretKey) -> Scalar32Bytes {
    Scalar32Bytes::from_bytes(s.as_bytes()).unwrap()
}

#[test]
fn ristretto_group_operations_match_native() {
    let mut test = setup();
    let (a, b) = (point(1), point(2));
    let s = scalar(3);

    let sum: RistrettoPublicKeyBytes =
        test.call_function(TEMPLATE, "ristretto_add", args![pk_bytes(&a), pk_bytes(&b)], vec![]);
    assert_eq!(sum, pk_bytes(&(&a + &b)));

    let diff: RistrettoPublicKeyBytes =
        test.call_function(TEMPLATE, "ristretto_sub", args![pk_bytes(&a), pk_bytes(&b)], vec![]);
    assert_eq!(diff, pk_bytes(&(&a - &b)));

    let neg: RistrettoPublicKeyBytes = test.call_function(TEMPLATE, "ristretto_negate", args![pk_bytes(&a)], vec![]);
    assert_eq!(neg, pk_bytes(&(&RistrettoPublicKey::default() - &a)));

    let product: RistrettoPublicKeyBytes =
        test.call_function(TEMPLATE, "ristretto_mul", args![pk_bytes(&a), sk_bytes(&s)], vec![]);
    assert_eq!(product, pk_bytes(&(&a * &s)));

    let base: RistrettoPublicKeyBytes = test.call_function(TEMPLATE, "ristretto_mul_base", args![sk_bytes(&s)], vec![]);
    assert_eq!(base, pk_bytes(&RistrettoPublicKey::from_secret_key(&s)));
}

#[test]
fn scalar_field_operations_match_native() {
    let mut test = setup();
    let (a, b) = (scalar(7), scalar(8));

    let sum: Scalar32Bytes = test.call_function(TEMPLATE, "scalar_add", args![sk_bytes(&a), sk_bytes(&b)], vec![]);
    assert_eq!(sum, sk_bytes(&(&a + &b)));

    let diff: Scalar32Bytes = test.call_function(TEMPLATE, "scalar_sub", args![sk_bytes(&a), sk_bytes(&b)], vec![]);
    assert_eq!(diff, sk_bytes(&(&a - &b)));

    let product: Scalar32Bytes = test.call_function(TEMPLATE, "scalar_mul", args![sk_bytes(&a), sk_bytes(&b)], vec![]);
    assert_eq!(product, sk_bytes(&(&a * &b)));

    let neg: Scalar32Bytes = test.call_function(TEMPLATE, "scalar_negate", args![sk_bytes(&a)], vec![]);
    assert_eq!(neg, sk_bytes(&(&RistrettoSecretKey::default() - &a)));
}

/// Inversion is the one scalar operation with a genuinely absent answer, and the absence is the
/// result rather than an error: zero is a well-formed scalar that simply has no inverse.
#[test]
fn scalar_inversion_returns_none_only_for_zero() {
    let mut test = setup();
    let a = scalar(11);

    let inverse: Option<Scalar32Bytes> = test.call_function(TEMPLATE, "scalar_invert", args![sk_bytes(&a)], vec![]);
    let inverse = inverse.expect("a non-zero scalar has an inverse");

    // The defining property, rather than a recomputation of the same formula.
    let product: Scalar32Bytes = test.call_function(TEMPLATE, "scalar_mul", args![sk_bytes(&a), inverse], vec![]);
    assert_eq!(product, sk_bytes(&RistrettoSecretKey::from(1u64)));

    let zero: Option<Scalar32Bytes> =
        test.call_function(TEMPLATE, "scalar_invert", args![Scalar32Bytes::zero()], vec![]);
    assert!(zero.is_none(), "zero has no inverse");
}

#[test]
fn hashes_match_native() {
    let mut test = setup();
    let data = b"tari ootle intrinsics".to_vec();

    let blake: Hash32 = test.call_function(TEMPLATE, "hash_blake2b", args![data.clone()], vec![]);
    let expected = blake2::Blake2b::<digest::consts::U32>::digest(&data);
    assert_eq!(blake.into_array(), <[u8; 32]>::from(expected));

    let sha: Hash32 = test.call_function(TEMPLATE, "hash_sha256", args![data.clone()], vec![]);
    assert_eq!(sha.into_array(), <[u8; 32]>::from(sha2::Sha256::digest(&data)));

    let keccak: Hash32 = test.call_function(TEMPLATE, "hash_keccak256", args![data.clone()], vec![]);
    assert_eq!(keccak.into_array(), <[u8; 32]>::from(sha3::Keccak256::digest(&data)));

    let sha512: Hash64 = test.call_function(TEMPLATE, "hash_sha512", args![data.clone()], vec![]);
    assert_eq!(sha512.into_array(), <[u8; 64]>::from(sha2::Sha512::digest(&data)));
}

/// The multi-part form must agree with hashing the concatenation, or a template walking a Merkle
/// path would compute a different root than the one that committed it.
#[test]
fn hashing_parts_matches_hashing_the_concatenation() {
    let mut test = setup();
    let matches: bool = test.call_function(
        TEMPLATE,
        "hash_parts_matches_concat",
        args![b"left".to_vec(), b"right".to_vec()],
        vec![],
    );
    assert!(matches);
}

/// `(a + b)*G == a*G + b*G`. Only holds if scalar addition, fixed-base multiplication and point
/// addition are all wired to the operation they claim.
#[test]
fn the_group_is_homomorphic_over_scalar_addition() {
    let mut test = setup();
    let holds: bool = test.call_function(
        TEMPLATE,
        "homomorphism",
        args![sk_bytes(&scalar(21)), sk_bytes(&scalar(22))],
        vec![],
    );
    assert!(holds);
}

/// The property any confidential scheme built on these primitives depends on:
/// `commit(v1,r1) + commit(v2,r2) == commit(v1+v2, r1+r2)`.
#[test]
fn commitments_add() {
    let mut test = setup();
    let h = point(99);
    let holds: bool = test.call_function(
        TEMPLATE,
        "commitments_are_homomorphic",
        args![
            sk_bytes(&scalar(31)),
            sk_bytes(&scalar(32)),
            sk_bytes(&scalar(33)),
            sk_bytes(&scalar(34)),
            pk_bytes(&h)
        ],
        vec![],
    );
    assert!(holds);
}

/// A batched multi-scalar multiplication must produce exactly what the same terms produce one at a
/// time — it is priced as the cheaper path precisely because callers are expected to prefer it.
#[test]
fn msm_agrees_with_multiplying_each_term() {
    let mut test = setup();
    let points = vec![pk_bytes(&point(41)), pk_bytes(&point(42)), pk_bytes(&point(43))];
    let scalars = vec![sk_bytes(&scalar(51)), sk_bytes(&scalar(52)), sk_bytes(&scalar(53))];

    let agrees: bool = test.call_function(TEMPLATE, "msm_matches_loop", args![points, scalars], vec![]);
    assert!(agrees);
}

/// The boundary checks a template runs on untrusted bytes. Every other Ristretto intrinsic panics on
/// input these reject, so a template validates once here and then operates freely.
#[test]
fn validity_checks_separate_well_formed_input_from_garbage() {
    let mut test = setup();

    let canonical: bool = test.call_function(TEMPLATE, "ristretto_is_canonical", args![pk_bytes(&point(61))], vec![]);
    assert!(canonical);

    // All-ones is not a valid Ristretto encoding.
    let garbage = RistrettoPublicKeyBytes::from_bytes(&[0xFFu8; 32]).unwrap();
    let canonical: bool = test.call_function(TEMPLATE, "ristretto_is_canonical", args![garbage], vec![]);
    assert!(!canonical, "0xFF..FF is not a canonical Ristretto point");

    let identity: bool = test.call_function(
        TEMPLATE,
        "ristretto_is_identity",
        args![pk_bytes(&RistrettoPublicKey::default())],
        vec![],
    );
    assert!(identity);

    let identity: bool = test.call_function(TEMPLATE, "ristretto_is_identity", args![pk_bytes(&point(62))], vec![]);
    assert!(!identity);

    // A scalar at or above the group order is non-canonical, and is the malleability case worth
    // rejecting on anything a counterparty supplied.
    let canonical: bool = test.call_function(TEMPLATE, "scalar_is_canonical", args![sk_bytes(&scalar(63))], vec![]);
    assert!(canonical);
    let canonical: bool = test.call_function(
        TEMPLATE,
        "scalar_is_canonical",
        args![Scalar32Bytes::from_bytes(&[0xFFu8; 32]).unwrap()],
        vec![],
    );
    assert!(!canonical);
}

/// Malformed input aborts the transaction rather than returning a sentinel, which is what keeps the
/// common path free of `Result` threading.
#[test]
fn a_non_canonical_point_aborts_the_transaction() {
    let mut test = setup();
    let garbage = RistrettoPublicKeyBytes::from_bytes(&[0xFFu8; 32]).unwrap();
    let tx = test
        .transaction()
        .call_function(test.get_template_address(TEMPLATE), "ristretto_add", args![
            garbage,
            pk_bytes(&point(71))
        ])
        .build_and_seal(test.secret_key());

    let reason = test.execute_expect_failure(tx, vec![]);
    assert!(
        format!("{reason}").contains("canonical"),
        "expected the rejection to name the non-canonical point, got {reason}",
    );
}

/// An intrinsic is charged as native execution, on the same budget and at the same per-point rate
/// as WASM. The charge scales with the work — a multi-scalar multiplication over more terms costs
/// more — which is what makes the price a function of the declared arguments rather than a
/// constant.
///
/// Intrinsics get no `FeeSource` of their own: every kind of native work shares one, so the receipt
/// size bound does not grow to itemise a breakdown the shared per-point rate already makes
/// comparable.
#[test]
fn intrinsics_are_charged_as_native_execution() {
    let mut test = setup();
    let (account, owner, key) = test.create_funded_account();
    test.enable_fees();

    let mut msm_cost = |terms: u64| -> (u64, u64) {
        let points = (1..=terms).map(|i| sk_bytes(&scalar(i))).collect::<Vec<_>>();
        let pts = (1..=terms).map(|i| pk_bytes(&point(i))).collect::<Vec<_>>();
        let tx = Transaction::builder_localnet(Epoch(1))
            .pay_fee_from_component(account, 900_000_000u64)
            .call_function(test.get_template_address(TEMPLATE), "ristretto_msm", args![pts, points])
            .build_and_seal(&key);
        let result = test.execute_expect_success(tx, vec![owner.clone()]);
        let native_fee = result
            .finalize
            .fee_receipt
            .fee_breakdown()
            .iter()
            .find_map(|(source, amount)| (*source == FeeSource::NativeExecution).then_some(*amount))
            .expect("an intrinsic must appear under NativeExecution");
        (result.native_execution_points, native_fee)
    };

    let (small_points, small_fee) = msm_cost(1);
    let (large_points, large_fee) = msm_cost(8);

    assert!(small_points > 0, "an intrinsic must consume native points");
    assert!(
        large_points > small_points,
        "an 8-term multi-scalar multiplication cost {large_points} points, not more than the {small_points} a 1-term \
         one cost",
    );
    assert!(small_fee > 0 && large_fee > small_fee, "the fee must follow the points");
}
