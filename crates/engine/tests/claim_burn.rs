//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine::traits::{ClaimProofError, ClaimProofRejection, ClaimProofVerifier, VerifiedClaim};
use tari_engine_types::{
    commit_result::ExecutionFailureCode,
    confidential::{
        BurnOutput,
        BurnOutputFeatures,
        BurnOutputInclusionProof,
        MinotariBurnClaimProof,
        MmrInclusionProof,
    },
};
use tari_ootle_transaction::{Epoch, Transaction, TransactionBuilder};
use tari_template_lib::types::{
    Amount,
    Hash32,
    constants::TARI_TOKEN,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes},
};
use tari_template_test_tooling::{TemplateTest, support::stealth, wallet_crypto::MaskAndValue};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

struct FixedOutcomeVerifier(ClaimProofError);

impl ClaimProofVerifier for FixedOutcomeVerifier {
    fn verify_claim_proof(
        &self,
        _epoch: Epoch,
        _claim: &MinotariBurnClaimProof,
    ) -> Result<VerifiedClaim, ClaimProofError> {
        Err(self.0.clone())
    }
}

/// Accepts every claim, as a claim whose ownership proof and output inclusion verify
struct AcceptingVerifier;

impl ClaimProofVerifier for AcceptingVerifier {
    fn verify_claim_proof(
        &self,
        _epoch: Epoch,
        claim: &MinotariBurnClaimProof,
    ) -> Result<VerifiedClaim, ClaimProofError> {
        Ok(VerifiedClaim {
            claim_public_key: claim.output.features.claim_public_key,
        })
    }
}

fn keypair() -> (RistrettoSecretKey, RistrettoPublicKeyBytes) {
    let (secret, public) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    (secret, public.to_byte_type())
}

/// A claim of a burn output made out to `claim_public_key`
fn claim_to(claim_public_key: RistrettoPublicKeyBytes) -> MinotariBurnClaimProof {
    let mut proof = claim();
    proof.output.features.claim_public_key = claim_public_key;
    proof
}

fn one_byte<const N: usize>() -> bounded_vec::BoundedVec<u8, 1, N> {
    bounded_vec::BoundedVec::<u8, 1, N>::from_vec(vec![0]).unwrap()
}

fn claim() -> MinotariBurnClaimProof {
    let mmr_proof = || MmrInclusionProof {
        leaf_index: 0,
        mmr_size: 1,
        path: vec![],
        peaks: vec![],
    };
    MinotariBurnClaimProof {
        commitment: PedersenCommitmentBytes::zero(),
        ownership_proof: SchnorrSignatureBytes::zero(),
        value: 1_000,
        output: BurnOutput {
            version: 0,
            features: BurnOutputFeatures {
                version: 0,
                maturity: 0,
                claim_public_key: RistrettoPublicKeyBytes::zero(),
                sidechain_id: None,
                range_proof_type: 0,
            },
            rangeproof_hash: Hash32::zero(),
            script: one_byte(),
            sender_offset_public_key: RistrettoPublicKeyBytes::zero(),
            metadata_signature: one_byte(),
            covenant: one_byte(),
            encrypted_data: one_byte(),
            minimum_value_promise: 0,
        },
        inclusion_proof: BurnOutputInclusionProof {
            block_hash: Hash32::zero(),
            normal_output_proof: mmr_proof(),
            normal_output_mr: Hash32::zero(),
            block_output_proof: mmr_proof(),
        },
    }
}

/// Claims `proof` and pays the claimed bucket into a single stealth output
fn claim_into_output<D>(builder: TransactionBuilder<D>, proof: MinotariBurnClaimProof) -> TransactionBuilder<D> {
    let transfer = stealth::generate_transfer_data(
        stealth::NO_INPUTS,
        Amount::from(proof.value),
        [proof.value],
        Amount::zero(),
    );
    builder
        .claim_burn(proof)
        .put_last_instruction_output_on_workspace("claimed")
        .stealth_transfer_with_input_bucket(TARI_TOKEN, transfer.statement, "claimed")
}

fn verifier_fault() -> FixedOutcomeVerifier {
    FixedOutcomeVerifier(ClaimProofError::VerifierFault("database unavailable".to_string()))
}

#[test]
fn a_verifier_fault_in_the_fee_intent_aborts_execution() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(verifier_fault());

    let err = test
        .try_execute(
            Transaction::builder_localnet(Epoch(1))
                .with_fee_instructions_builder(|builder| claim_into_output(builder, claim()))
                .build_and_seal(test.secret_key()),
            vec![],
        )
        .expect_err("a verifier fault must not produce a receipt");
    assert!(err.is_node_fault(), "expected a node fault, got {err}");
}

#[test]
fn a_verifier_fault_in_the_main_intent_aborts_execution() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(verifier_fault());

    let err = test
        .try_execute(
            claim_into_output(Transaction::builder_localnet(Epoch(1)), claim()).build_and_seal(test.secret_key()),
            vec![],
        )
        .expect_err("a verifier fault must not produce a receipt");
    assert!(err.is_node_fault(), "expected a node fault, got {err}");
}

#[test]
fn an_invalid_claim_is_rejected_with_a_receipt() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(FixedOutcomeVerifier(
        ClaimProofRejection::Invalid("bad proof".to_string()).into(),
    ));

    let result = test
        .try_execute(
            claim_into_output(Transaction::builder_localnet(Epoch(1)), claim()).build_and_seal(test.secret_key()),
            vec![],
        )
        .expect("an invalid claim is a verdict on the transaction");
    let reason = result.finalize.any_reject().expect("an invalid claim is rejected");
    assert_eq!(
        reason.execution_failure_code(),
        Some(ExecutionFailureCode::InvalidProof),
        "unexpected reject: {reason}"
    );
}

#[test]
fn a_claim_sealed_by_the_claim_key_is_accepted() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(AcceptingVerifier);
    let (claim_secret, claim_public_key) = keypair();

    let result = test
        .try_execute(
            claim_into_output(Transaction::builder_localnet(Epoch(1)), claim_to(claim_public_key))
                .build_and_seal(&claim_secret),
            vec![],
        )
        .unwrap();
    if let Some(reason) = result.finalize.any_reject() {
        panic!("a claim sealed by the claim key must be accepted: {reason}");
    }
}

#[test]
fn a_claim_co_signed_by_the_claim_key_is_accepted() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(AcceptingVerifier);
    let (claim_secret, claim_public_key) = keypair();

    // Another key, e.g. a relayer paying the fee, seals
    let result = test
        .try_execute(
            claim_into_output(Transaction::builder_localnet(Epoch(1)), claim_to(claim_public_key))
                .add_signer(&test.to_public_key_bytes(), &claim_secret)
                .seal(test.secret_key()),
            vec![],
        )
        .unwrap();
    if let Some(reason) = result.finalize.any_reject() {
        panic!("a claim co-signed by the claim key must be accepted: {reason}");
    }
}

/// The claim's value arrives as a revealed bucket for the claim transaction to move on, so the only UTXO is the one
/// the transaction pays it into.
#[test]
fn a_claim_reveals_the_burnt_value_in_a_bucket() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(AcceptingVerifier);
    let (claim_secret, claim_public_key) = keypair();
    let proof = claim_to(claim_public_key);
    let value = proof.value;
    let transfer = stealth::generate_transfer_data(stealth::NO_INPUTS, Amount::from(value), [value], Amount::zero());

    let result = test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .claim_burn(proof)
            .put_last_instruction_output_on_workspace("claimed")
            .assert_bucket_contains_exactly("claimed", TARI_TOKEN, value)
            .stealth_transfer_with_input_bucket(TARI_TOKEN, transfer.statement, "claimed")
            .build_and_seal(&claim_secret),
        vec![],
    );
    let diff = result.finalize.any_accept().unwrap();
    assert_eq!(
        diff.up_iter()
            .filter(|(_, substate)| substate.substate_value().as_utxo().is_some())
            .count(),
        1
    );
}

#[test]
fn a_claim_that_leaves_its_bucket_behind_is_rejected() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    test.set_claim_proof_verifier(AcceptingVerifier);
    let (claim_secret, claim_public_key) = keypair();

    let result = test
        .try_execute(
            Transaction::builder_localnet(Epoch(1))
                .claim_burn(claim_to(claim_public_key))
                .build_and_seal(&claim_secret),
            vec![],
        )
        .unwrap();
    assert!(
        result.finalize.any_reject().is_some(),
        "a dangling claimed bucket must reject the claim"
    );
}

#[test]
fn the_burner_cannot_claim_a_burn_made_out_to_someone_else() {
    let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
    // The burner knows the commitment opening, so its ownership proof verifies
    test.set_claim_proof_verifier(AcceptingVerifier);
    let (_recipient_secret, recipient_public_key) = keypair();
    let (burner_secret, burner_public_key) = keypair();
    let proof = claim_to(recipient_public_key);

    for transaction in [
        claim_into_output(Transaction::builder_localnet(Epoch(1)), proof.clone()).build_and_seal(&burner_secret),
        claim_into_output(Transaction::builder_localnet(Epoch(1)), proof)
            .add_signer(&burner_public_key, test.secret_key())
            .seal(&burner_secret),
    ] {
        let result = test
            .try_execute(transaction, vec![])
            .expect("an unauthorized claim is a verdict on the transaction");
        let reason = result
            .finalize
            .any_reject()
            .expect("a claim without the claim key's signature must be rejected");
        assert_eq!(
            reason.execution_failure_code(),
            Some(ExecutionFailureCode::AccessDenied),
            "unexpected reject: {reason}"
        );
    }
}

/// The burner knows the burn commitment's opening, so they can put a stealth output of their own at the burn
/// commitment, and spend it again, before the recipient claims. Neither may stand in the way of the claim.
mod occupied_commitment {
    use std::collections::HashSet;

    use tari_engine_types::substate::SubstateId;
    use tari_ootle_transaction::args;
    use tari_template_lib::types::{
        ComponentAddress,
        EncryptedData,
        NonFungibleAddress,
        UtxoAddress,
        crypto::UtxoTag,
        stealth::{RevealedOutput, SpendAuthorization},
    };
    use tari_template_test_tooling::wallet_crypto::{
        OutputWitness,
        StealthInputWitness,
        StealthOutputWitness,
        stealth::create_transfer_statement,
    };

    use super::*;

    const BURNT_VALUE: u64 = 500_000;

    struct Burner {
        account: ComponentAddress,
        proof: NonFungibleAddress,
        secret: RistrettoSecretKey,
        public_key: RistrettoPublicKeyBytes,
    }

    /// The burner funds a stealth output with their own L2 TARI whose commitment is the burn commitment, returning
    /// the substates that transaction created.
    fn occupy(test: &mut TemplateTest, burner: &Burner, opening: &MaskAndValue) -> HashSet<SubstateId> {
        let output = StealthOutputWitness {
            witness: OutputWitness {
                amount: opening.value,
                mask: opening.mask.clone(),
                sender_public_nonce: stealth::test_sender_public_nonce(),
                minimum_value_promise: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                resource_view_key: None,
            },
            auth: SpendAuthorization::Key(burner.public_key),
            tag: UtxoTag::new(0),
        };
        let statement = create_transfer_statement(
            std::iter::empty::<StealthInputWitness>(),
            Amount::from(opening.value),
            [&output],
            None,
        )
        .unwrap();
        let result = test.execute_expect_success(
            Transaction::builder_localnet(Epoch(1))
                .call_method(burner.account, "withdraw", args![TARI_TOKEN, opening.value])
                .put_last_instruction_output_on_workspace("bucket")
                .stealth_transfer_with_input_bucket(TARI_TOKEN, statement, "bucket")
                .build_and_seal(&burner.secret),
            vec![burner.proof.clone()],
        );
        let created = result
            .finalize
            .any_accept()
            .unwrap()
            .up_iter()
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        let occupied = SubstateId::Utxo(UtxoAddress::new(
            TARI_TOKEN,
            opening.to_commitment().to_byte_type().into(),
        ));
        assert!(
            created.contains(&occupied),
            "the burner's output must sit at the burn commitment"
        );
        created
    }

    /// The burner spends the output at the burn commitment back into their account.
    fn release(test: &mut TemplateTest, burner: &Burner, opening: &MaskAndValue) {
        let statement = create_transfer_statement(
            [StealthInputWitness::new(opening.clone())],
            Amount::zero(),
            std::iter::empty::<&StealthOutputWitness>(),
            Some(RevealedOutput::new(Amount::from(opening.value), burner.public_key)),
        )
        .unwrap();
        test.execute_expect_success(
            Transaction::builder_localnet(Epoch(1))
                .stealth_transfer(TARI_TOKEN, statement)
                .put_last_instruction_output_on_workspace("released")
                .call_method(burner.account, "deposit", args![Workspace("released")])
                .build_and_seal(&burner.secret),
            vec![burner.proof.clone()],
        );
    }

    fn setup() -> (TemplateTest, Burner, MaskAndValue) {
        let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
        test.set_claim_proof_verifier(AcceptingVerifier);
        let (account, proof, secret, public_key) = test.create_funded_account_with_keypair();
        let burner = Burner {
            account,
            proof,
            secret,
            public_key: public_key.to_byte_type(),
        };
        let opening = MaskAndValue {
            mask: keypair().0,
            value: BURNT_VALUE,
        };
        (test, burner, opening)
    }

    fn claim_of(opening: &MaskAndValue, claim_public_key: RistrettoPublicKeyBytes) -> MinotariBurnClaimProof {
        let mut proof = claim_to(claim_public_key);
        proof.commitment = opening.to_commitment().to_byte_type();
        proof.value = opening.value;
        proof
    }

    #[test]
    fn a_live_output_at_the_burn_commitment_does_not_block_the_claim() {
        let (mut test, burner, opening) = setup();
        let (recipient_secret, recipient_public_key) = keypair();
        occupy(&mut test, &burner, &opening);

        let result = test
            .try_execute(
                claim_into_output(
                    Transaction::builder_localnet(Epoch(1)),
                    claim_of(&opening, recipient_public_key),
                )
                .build_and_seal(&recipient_secret),
                vec![],
            )
            .unwrap();
        if let Some(reason) = result.finalize.any_reject() {
            panic!("the recipient's claim must be accepted: {reason}");
        }
    }

    #[test]
    fn a_spent_output_at_the_burn_commitment_does_not_block_the_claim() {
        let (mut test, burner, opening) = setup();
        let (recipient_secret, recipient_public_key) = keypair();
        let ever_created = occupy(&mut test, &burner, &opening);
        release(&mut test, &burner, &opening);

        // Consensus reports every substate that was ever created as taken, spent or not
        let (result, _) = test
            .try_execute_with_fee_intent_fallback(
                Transaction::builder_localnet(Epoch(1))
                    .with_fee_instructions_builder(|builder| {
                        claim_into_output(builder, claim_of(&opening, recipient_public_key))
                    })
                    .build_and_seal(&recipient_secret),
                vec![],
                &|id: &SubstateId| ever_created.contains(id),
            )
            .unwrap();
        if let Some(reason) = result.finalize.any_reject() {
            panic!("the recipient's claim must be accepted: {reason}");
        }
    }
}

/// Holds `ClaimBurnShape::estimate_fee` to what the engine charges the claim a wallet builds.
mod fee_estimate {
    use tari_crypto::keys::SecretKey as _;
    use tari_engine::fees::FeeTable;
    use tari_engine_types::{
        fees::{FeeRates, FeeScale, FeeSource},
        stealth::{ClaimBurnShape, persisted_utxo_bytes},
    };
    use tari_template_lib::types::stealth::StealthTransferStatement;

    use super::*;

    /// What the burn being claimed is worth. Comfortably above any claim's fee, so the fee is never
    /// the binding constraint on it.
    const CLAIMED_VALUE: u64 = 10_000_000;

    /// The burn every shipped network is configured with: a share of the payment, so it must move
    /// nothing the estimate prices.
    const BURN_RATE_BPS: u16 = 500;

    /// The margin the estimate may carry over the real charge. The tombstone's value and the
    /// receipt's epoch are priced at their widest encodings, which leaves a handful of microtari, and
    /// a claim cannot get back what it reveals over the charge.
    const MAX_OVERSHOOT: u64 = 12;

    /// Stands in for the fee while the claim is built only to be weighed.
    const PLACEHOLDER_FEE: u64 = 1_000_000;

    fn setup() -> TemplateTest {
        let mut test = TemplateTest::new(CRATE_PATH, &[] as &[&str]);
        test.set_claim_proof_verifier(AcceptingVerifier);
        // Price a created substate as the shipped tables do, so the slots the claim creates carry the
        // weight they do on a real network.
        test.set_fee_table(FeeTable {
            per_substate_create_cost: 250,
            ..test.fee_table().clone()
        });
        test.enable_fees();
        test.set_burn_rate_bps(BURN_RATE_BPS);
        test
    }

    fn rates(test: &TemplateTest) -> FeeRates {
        test.fee_table().to_rates(FeeScale::ONE)
    }

    fn new_claim() -> MaskAndValue {
        MaskAndValue::new(CLAIMED_VALUE, RistrettoSecretKey::random(&mut rand::rng()))
    }

    struct Build {
        transaction: Transaction,
        statement: StealthTransferStatement,
    }

    /// Builds the transaction a wallet builds to claim a burn: claim it, pay the claimed bucket into one
    /// stealth output revealing `revealed_fee`, and pay the revealed bucket as the fee. The claim key seals, so
    /// the claim and the revealed output answer to one signer.
    fn build(test: &TemplateTest, claimed: &MaskAndValue, revealed_fee: u64) -> Build {
        let mut proof = claim_to(test.to_public_key_bytes());
        proof.commitment = claimed.to_commitment().to_byte_type();
        proof.value = claimed.value;

        let transfer = stealth::generate_transfer_data(
            stealth::NO_INPUTS,
            Amount::from(claimed.value),
            [claimed.value - revealed_fee],
            Amount::from(revealed_fee),
        );
        let transaction = Transaction::builder_localnet(Epoch(1))
            .with_fee_instructions_builder(|builder| {
                builder
                    .claim_burn(proof)
                    .put_last_instruction_output_on_workspace("claimed")
                    .stealth_transfer_with_input_bucket(TARI_TOKEN, transfer.statement.clone(), "claimed")
                    .put_last_instruction_output_on_workspace("fee")
                    .pay_fee_from_bucket("fee")
            })
            .build_and_seal(test.secret_key());
        Build {
            transaction,
            statement: transfer.statement,
        }
    }

    /// The groups beyond the first that a claim's declared inputs are taken to be pledged to: the TARI
    /// resource and the claim's tombstone in two groups.
    const INPUT_FANOUT: u32 = 1;

    fn shape_of(test: &TemplateTest, build: &Build) -> ClaimBurnShape {
        ClaimBurnShape {
            persisted_output_bytes: build
                .statement
                .outputs_statement
                .outputs
                .iter()
                .map(persisted_utxo_bytes)
                .sum(),
            transaction_weight: build.transaction.calculate_transaction_weight().as_u64(),
            input_bytes: test.declared_input_bytes(&build.transaction),
            input_fanout: INPUT_FANOUT,
        }
    }

    #[test]
    fn the_estimate_bounds_what_the_engine_charges_a_claim() {
        let mut test = setup();
        test.set_input_fanout(INPUT_FANOUT);
        let claimed = new_claim();

        let shape = shape_of(&test, &build(&test, &claimed, PLACEHOLDER_FEE));
        let settled = build(&test, &claimed, shape.estimate_fee(&rates(&test)));
        assert_eq!(
            shape_of(&test, &settled),
            shape,
            "the fee a claim reveals must not move the shape it is priced at"
        );

        // A bucket-paid fee is not refunded, so the claim commits only if the estimate covered the
        // charge, and whatever it did not need is recorded as the overcharge.
        let result = test.execute_expect_success(settled.transaction, vec![]);
        let receipt = result.finalize.fee_receipt;
        assert!(
            receipt.is_paid_in_full(),
            "{shape:?} left {} unpaid",
            receipt.unpaid_debt()
        );
        let overshoot = receipt.total_fee_overcharge();
        assert!(
            overshoot <= MAX_OVERSHOOT,
            "{shape:?} was charged {} and overpaid {overshoot}",
            receipt.total_fees_charged(),
        );
    }

    /// The estimate's host-call count is a constant standing in for an instruction sequence it cannot
    /// see. Reading the charge back at a known per-call rate pins it to what the engine counted.
    #[test]
    fn the_runtime_call_count_matches_the_claim_sequence() {
        let mut test = setup();
        let mut fee_table = FeeTable::zero_rated();
        fee_table.per_module_call_cost = 1;
        test.set_fee_table(fee_table);
        test.set_burn_rate_bps(0);

        let result = test.execute_expect_success(build(&test, &new_claim(), PLACEHOLDER_FEE).transaction, vec![]);
        assert_eq!(
            result.finalize.fee_receipt.fee_breakdown().get(FeeSource::RuntimeCall),
            ClaimBurnShape::RUNTIME_CALLS,
        );
    }

    /// Reading the native charge back with only native points priced pins the claim's native points to every point
    /// the engine charged, verification, authorization and argument resolution alike. Every price is a multiple of
    /// the divisor, the per-byte argument price the smallest, so no point is lost to rounding. The claim reveals
    /// half its value, enough to cover the charge at that divisor.
    #[test]
    fn the_native_points_match_the_claim_shape() {
        const DIVISOR: u64 = tari_engine_types::limits::NativeExecutionPoints::PER_ENGINE_ARG_BYTE;
        let mut test = setup();
        let mut fee_table = FeeTable::zero_rated();
        fee_table.per_wasm_point_cost = 1;
        fee_table.wasm_points_cost_divisor = DIVISOR;
        test.set_fee_table(fee_table);
        test.set_burn_rate_bps(0);

        let result = test.execute_expect_success(build(&test, &new_claim(), CLAIMED_VALUE / 2).transaction, vec![]);
        assert_eq!(
            result
                .finalize
                .fee_receipt
                .fee_breakdown()
                .get(FeeSource::NativeExecution) *
                DIVISOR,
            ClaimBurnShape::native_points(),
        );
    }

    /// The bound rests on an underpaid claim being rejected. Revealing half the estimate shows that it
    /// is, so the bound is not passing vacuously.
    #[test]
    fn revealing_under_the_charge_rejects_the_claim() {
        let mut test = setup();
        let claimed = new_claim();
        let shape = shape_of(&test, &build(&test, &claimed, PLACEHOLDER_FEE));
        let too_little = shape.estimate_fee(&rates(&test)) / 2;

        let result = test
            .try_execute(build(&test, &claimed, too_little).transaction, vec![])
            .unwrap();
        assert!(
            result.finalize.fee_receipt.unpaid_debt() > 0,
            "revealing {too_little} should have left a debt"
        );
        result.expect_failure();
    }
}
