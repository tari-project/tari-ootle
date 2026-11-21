//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod support;

use std::time::Duration;

use ootle_byte_type::ToByteType;
use tari_consensus_types::Decision;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{
    Epoch,
    commit_result::{AbortReason, ExecuteResult, FinalizeResult, RejectReason, TransactionResult},
    crypto::commit_amount,
    fees::{FeeBreakdown, FeeReceipt, FeeSource},
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{SubstateVersion, optional::IsNotFoundError};
use tari_ootle_transaction::{Transaction, TransactionId, args};
use tari_ootle_wallet_sdk::{
    apis::transaction::{Submission, TRANSACTION_EXPIRY_GRACE_EPOCHS},
    models::{TransactionStatus, WalletLockId, WalletTransactionUpdate},
    network::{TransactionFinalizedResult, TransactionQueryResult},
    storage::{ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_template_lib::types::{ConfidentialOutputAddress, constants::STEALTH_TARI_RESOURCE_ADDRESS};
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::support::{CannedTransactionResultInterface, Test, TestWithNetwork};

fn now() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

fn build_transaction() -> Transaction {
    Transaction::builder_localnet(Epoch(100))
        .allocate_component_address("component")
        .put_last_instruction_output_on_workspace("bucket")
        .call_method("component", "new", args!["bucket"])
        .build_and_seal(&RistrettoSecretKey::from(1u64))
}

/// A fee-only accept where the fee intent could not cover everything charged: 1000 was paid up front and all of it
/// was consumed, but the execution charged 2548.
fn underpaid_fee_receipt() -> FeeReceipt {
    let mut cost_breakdown = FeeBreakdown::default();
    cost_breakdown.add(FeeSource::Initial, 2548);

    FeeReceipt::builder()
        .with_total_fee_payment(1000)
        .with_total_fees_paid(1000)
        .with_cost_breakdown(cost_breakdown)
        .build()
}

fn finalized_result(transaction_id: TransactionId, fee_receipt: FeeReceipt) -> TransactionQueryResult {
    let finalize = FinalizeResult::new(
        transaction_id.into_array().into(),
        vec![],
        vec![],
        TransactionResult::AcceptFeeRejectRest(
            SubstateDiff::new(),
            RejectReason::execution_failure_unclassified("out of fees"),
        ),
        fee_receipt,
    );

    TransactionQueryResult {
        transaction_id,
        result: TransactionFinalizedResult::Finalized {
            final_decision: Decision::Commit,
            execution_result: Some(Box::new(ExecuteResult {
                finalize,
                execution_time: Duration::from_secs(1),
                execute_epoch: None,
                wasm_execution_points: 0,
                native_execution_points: 0,
            })),
            execution_time: Duration::from_secs(1),
            finalized_time: now(),
            abort_details: None,
        },
        receipt: None,
        encoded_execution_result: None,
    }
}

#[tokio::test]
async fn final_fee_is_what_was_paid_not_what_was_required() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    let fee_receipt = underpaid_fee_receipt();
    // `required_fees()` is a dry run estimate for a later submit, never what a settled transaction cost, so the two
    // must be distinguishable here.
    assert_ne!(fee_receipt.total_fees_paid(), fee_receipt.required_fees());

    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::new(finalized_result(
        transaction_id,
        fee_receipt.clone(),
    )));
    test.store()
        .with_write_tx(|tx| tx.transactions_insert(&transaction, None, &[Test::test_account_address()], false))
        .unwrap();

    let finalized = test
        .sdk()
        .transaction_api()
        .check_and_store_finalized_transaction(transaction_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(finalized.status, TransactionStatus::OnlyFeeAccepted);
    assert_eq!(finalized.final_fee, Some(fee_receipt.total_fees_paid()));
}

#[tokio::test]
async fn final_fee_is_zero_for_a_rejected_transaction() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();

    let mut query_result = finalized_result(transaction_id, FeeReceipt::default());
    let TransactionFinalizedResult::Finalized {
        final_decision,
        execution_result,
        ..
    } = &mut query_result.result
    else {
        unreachable!()
    };
    *final_decision = Decision::Abort(AbortReason::InsufficientFeesPaid);
    execution_result.as_mut().unwrap().finalize.result =
        TransactionResult::Reject(RejectReason::execution_failure_unclassified("nope"));

    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::new(query_result));
    test.store()
        .with_write_tx(|tx| tx.transactions_insert(&transaction, None, &[Test::test_account_address()], false))
        .unwrap();

    let finalized = test
        .sdk()
        .transaction_api()
        .check_and_store_finalized_transaction(transaction_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(finalized.status, TransactionStatus::Rejected);
    assert_eq!(finalized.final_fee, Some(0));
}

#[tokio::test]
async fn an_abort_releases_the_locked_outputs_whatever_its_execution_accepted() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();

    // The execution before the abort decision accepted a diff that spends both outputs the transaction locks.
    let mut spends = SubstateDiff::new();
    for amount in [25u64, 49] {
        let commitment = commit_amount(&Default::default(), amount.into())
            .unwrap()
            .to_byte_type();
        spends.down(
            SubstateId::ConfidentialOutput(ConfidentialOutputAddress::new(
                STEALTH_TARI_RESOURCE_ADDRESS,
                commitment,
            )),
            SubstateVersion::ZERO,
        );
    }
    let mut query_result = finalized_result(transaction_id, FeeReceipt::default());
    let TransactionFinalizedResult::Finalized {
        final_decision,
        execution_result,
        ..
    } = &mut query_result.result
    else {
        unreachable!()
    };
    *final_decision = Decision::Abort(AbortReason::InsufficientFeesPaid);
    execution_result.as_mut().unwrap().finalize.result = TransactionResult::Accept(spends);

    let test = wallet_with_inputs_locked_for_new(CannedTransactionResultInterface::new(query_result), &transaction);

    let finalized = test
        .sdk()
        .transaction_api()
        .check_and_store_finalized_transaction(transaction_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(finalized.status, TransactionStatus::Rejected);
    assert_eq!(test.get_unspent_balance(), 74, "both outputs are released, not spent");
}

#[tokio::test]
async fn a_committed_transaction_finalizes_all_of_its_locks() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();

    let mut spends = SubstateDiff::new();
    for amount in [25u64, 49] {
        let commitment = commit_amount(&Default::default(), amount.into())
            .unwrap()
            .to_byte_type();
        spends.down(
            SubstateId::ConfidentialOutput(ConfidentialOutputAddress::new(
                STEALTH_TARI_RESOURCE_ADDRESS,
                commitment,
            )),
            SubstateVersion::ZERO,
        );
    }
    let mut query_result = finalized_result(transaction_id, FeeReceipt::default());
    let TransactionFinalizedResult::Finalized { execution_result, .. } = &mut query_result.result else {
        unreachable!()
    };
    execution_result.as_mut().unwrap().finalize.result = TransactionResult::Accept(spends);

    let test = wallet_with_inputs_locked_for(CannedTransactionResultInterface::new(query_result), &transaction);

    let finalized = test
        .sdk()
        .transaction_api()
        .check_and_store_finalized_transaction(transaction_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(finalized.status, TransactionStatus::Accepted);
    assert_eq!(test.get_unspent_balance(), 0);
    assert!(
        test.sdk()
            .locks_api()
            .get_locks_by_transaction_id(transaction_id)
            .unwrap()
            .is_empty()
    );
}

fn pending_result(transaction_id: TransactionId) -> TransactionQueryResult {
    TransactionQueryResult {
        transaction_id,
        result: TransactionFinalizedResult::Pending,
        receipt: None,
        encoded_execution_result: None,
    }
}

/// A wallet on `network` holding `transaction` as submitted, with outputs of 25 and 49 locked for it.
fn wallet_with_inputs_locked_for(
    network: CannedTransactionResultInterface,
    transaction: &Transaction,
) -> TestWithNetwork<CannedTransactionResultInterface> {
    let test = wallet_with_inputs_locked_for_new(network, transaction);
    test.store()
        .with_write_tx(|tx| {
            tx.transactions_update(
                WalletTransactionUpdate::new(transaction.calculate_id()).with_new_status(TransactionStatus::Pending),
            )
        })
        .unwrap();
    test
}

/// A wallet on `network` holding `transaction` not yet submitted, with outputs of 25 and 49 locked under separate
/// locks for it.
fn wallet_with_inputs_locked_for_new(
    network: CannedTransactionResultInterface,
    transaction: &Transaction,
) -> TestWithNetwork<CannedTransactionResultInterface> {
    let test = TestWithNetwork::with_network(network);
    test.add_unspent_output(25u64);
    test.add_unspent_output(49u64);
    let first_lock = test.new_lock().keep_locked();
    test.sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(first_lock, &Test::test_vault_address(), 25u64)
        .unwrap();
    let second_lock = test.new_lock().keep_locked();
    test.sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(second_lock, &Test::test_vault_address(), 49u64)
        .unwrap();
    test.sdk()
        .transaction_api()
        .insert_new_transaction(transaction.clone(), None, &[Test::test_account_address()], false, &[
            first_lock,
            second_lock,
        ])
        .unwrap();
    assert_eq!(test.get_unspent_balance(), 0);
    test
}

#[tokio::test]
async fn a_transaction_with_no_outcome_past_its_validity_window_expires_and_releases_its_inputs() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    // The transaction's max_epoch is 100.
    let expired_at = Epoch(100 + TRANSACTION_EXPIRY_GRACE_EPOCHS + 1);

    for network in [
        CannedTransactionResultInterface::new(pending_result(transaction_id)),
        CannedTransactionResultInterface::not_found(),
    ] {
        let test = wallet_with_inputs_locked_for(network.with_current_epoch(expired_at), &transaction);

        let expired = test
            .sdk()
            .transaction_api()
            .check_and_store_finalized_transaction(transaction_id)
            .await
            .unwrap()
            .expect("the transaction expired");

        assert_eq!(expired.status, TransactionStatus::InvalidTransaction);
        assert!(expired.finalize.is_none());
        assert!(expired.invalid_reason.is_some());
        assert_eq!(test.get_unspent_balance(), 74, "both outputs are released");
    }
}

#[tokio::test]
async fn a_transaction_with_no_outcome_stays_pending_while_the_network_may_still_finalize_it() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    let last_finalizable = Epoch(100 + TRANSACTION_EXPIRY_GRACE_EPOCHS);

    for network in [
        CannedTransactionResultInterface::new(pending_result(transaction_id)),
        CannedTransactionResultInterface::not_found(),
    ] {
        let test = wallet_with_inputs_locked_for(network.with_current_epoch(last_finalizable), &transaction);

        let pending = test
            .sdk()
            .transaction_api()
            .check_and_store_finalized_transaction(transaction_id)
            .await
            .unwrap();

        assert!(pending.is_none());
        let stored = test.sdk().transaction_api().get(transaction_id).unwrap();
        assert_eq!(stored.status, TransactionStatus::Pending);
        assert_eq!(test.get_unspent_balance(), 0, "the outputs stay locked");
    }
}

#[tokio::test]
async fn a_rejected_submission_releases_its_inputs() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    let test = wallet_with_inputs_locked_for_new(
        CannedTransactionResultInterface::not_found().rejecting_submissions(),
        &transaction,
    );

    let submission = test
        .sdk()
        .transaction_api()
        .submit_transaction(transaction_id)
        .await
        .unwrap();

    assert_eq!(submission, Submission::Rejected);
    let stored = test.sdk().transaction_api().get(transaction_id).unwrap();
    assert_eq!(stored.status, TransactionStatus::InvalidTransaction);
    assert_eq!(test.get_unspent_balance(), 74, "both outputs are released");
}

/// A wallet on `network` holding `transaction` as `New`, with outputs of 25 and 49 locked for it under two locks.
fn wallet_with_two_locks_for(
    network: CannedTransactionResultInterface,
    transaction: &Transaction,
) -> (TestWithNetwork<CannedTransactionResultInterface>, [WalletLockId; 2]) {
    let test = TestWithNetwork::with_network(network);
    let transaction_id = transaction.calculate_id();
    test.add_unspent_output(25u64);
    test.add_unspent_output(49u64);
    test.store()
        .with_write_tx(|tx| tx.transactions_insert(transaction, None, &[Test::test_account_address()], false))
        .unwrap();
    let lock_ids = [(); 2].map(|_| {
        let lock_id = test.new_lock().keep_locked();
        test.sdk()
            .confidential_outputs_api()
            .lock_outputs_by_amount(lock_id, &Test::test_vault_address(), 20u64)
            .unwrap();
        test.sdk()
            .transaction_api()
            .locks_set_transaction_id(lock_id, transaction_id)
            .unwrap();
        lock_id
    });
    assert_eq!(test.get_unspent_balance(), 0, "each lock holds one output");
    (test, lock_ids)
}

fn assert_locks_resolved(test: &TestWithNetwork<CannedTransactionResultInterface>, lock_ids: [WalletLockId; 2]) {
    for lock_id in lock_ids {
        let err = test.store().with_read_tx(|tx| tx.locks_get(lock_id)).unwrap_err();
        assert!(err.is_not_found_error(), "lock {lock_id} is resolved, got: {err}");
    }
}

#[tokio::test]
async fn a_rejected_submission_releases_every_lock_linked_to_it() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    let (test, lock_ids) = wallet_with_two_locks_for(
        CannedTransactionResultInterface::not_found().rejecting_submissions(),
        &transaction,
    );

    let submission = test
        .sdk()
        .transaction_api()
        .submit_transaction(transaction_id)
        .await
        .unwrap();

    assert_eq!(submission, Submission::Rejected);
    assert_eq!(test.get_unspent_balance(), 74, "the outputs of both locks are released");
    assert_locks_resolved(&test, lock_ids);
}

#[tokio::test]
async fn a_committed_transaction_spends_the_outputs_of_every_lock_linked_to_it() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();

    let mut spends = SubstateDiff::new();
    for amount in [25u64, 49] {
        let commitment = commit_amount(&Default::default(), amount.into())
            .unwrap()
            .to_byte_type();
        spends.down(
            SubstateId::ConfidentialOutput(ConfidentialOutputAddress::new(
                STEALTH_TARI_RESOURCE_ADDRESS,
                commitment,
            )),
            SubstateVersion::ZERO,
        );
    }
    let mut query_result = finalized_result(transaction_id, FeeReceipt::default());
    let TransactionFinalizedResult::Finalized { execution_result, .. } = &mut query_result.result else {
        unreachable!()
    };
    execution_result.as_mut().unwrap().finalize.result = TransactionResult::Accept(spends);
    let (test, lock_ids) = wallet_with_two_locks_for(CannedTransactionResultInterface::new(query_result), &transaction);
    test.store()
        .with_write_tx(|tx| {
            tx.transactions_update(
                WalletTransactionUpdate::new(transaction_id).with_new_status(TransactionStatus::Pending),
            )
        })
        .unwrap();

    let finalized = test
        .sdk()
        .transaction_api()
        .check_and_store_finalized_transaction(transaction_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(finalized.status, TransactionStatus::Accepted);
    assert_locks_resolved(&test, lock_ids);
    assert_eq!(
        test.get_unspent_balance(),
        0,
        "the outputs of both locks are spent, not released"
    );
}

#[tokio::test]
async fn the_locks_of_a_transaction_the_wallet_never_stored_are_released() {
    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::not_found());
    let transaction_id = build_transaction().calculate_id();
    test.add_unspent_output(25u64);
    let lock_id = test.new_lock().keep_locked();
    test.sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_id, &Test::test_vault_address(), 20u64)
        .unwrap();
    test.sdk()
        .transaction_api()
        .locks_set_transaction_id(lock_id, transaction_id)
        .unwrap();
    assert_eq!(test.get_unspent_balance(), 0);

    test.sdk()
        .transaction_api()
        .release_locks_of_unstored_transaction(transaction_id)
        .unwrap();

    assert_eq!(test.get_unspent_balance(), 25);
}

#[tokio::test]
async fn the_locks_of_a_stored_transaction_are_left_for_it_to_resolve() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    let (test, lock_ids) = wallet_with_two_locks_for(CannedTransactionResultInterface::not_found(), &transaction);

    test.sdk()
        .transaction_api()
        .release_locks_of_unstored_transaction(transaction_id)
        .unwrap();

    assert_eq!(test.get_unspent_balance(), 0, "the outputs stay locked");
    assert_eq!(
        test.sdk()
            .locks_api()
            .get_locks_by_transaction_id(transaction_id)
            .unwrap(),
        lock_ids.to_vec()
    );
}

#[tokio::test]
async fn release_unlinked_lock_releases_a_lock_no_transaction_holds() {
    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::not_found());
    test.add_unspent_output(25u64);
    let lock_id = test.new_lock().keep_locked();
    test.sdk()
        .confidential_outputs_api()
        .lock_outputs_by_amount(lock_id, &Test::test_vault_address(), 20u64)
        .unwrap();
    assert_eq!(test.get_unspent_balance(), 0);

    assert!(test.sdk().locks_api().release_unlinked_lock(lock_id).unwrap());

    assert_eq!(test.get_unspent_balance(), 25);
    assert!(
        !test.sdk().locks_api().release_unlinked_lock(lock_id).unwrap(),
        "a released lock no longer exists"
    );
}

#[tokio::test]
async fn release_unlinked_lock_leaves_a_lock_linked_to_a_stored_transaction() {
    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();
    let (test, lock_ids) = wallet_with_two_locks_for(CannedTransactionResultInterface::not_found(), &transaction);

    for lock_id in lock_ids {
        assert!(!test.sdk().locks_api().release_unlinked_lock(lock_id).unwrap());
    }

    assert_eq!(test.get_unspent_balance(), 0, "the outputs stay locked");
    assert_eq!(
        test.sdk()
            .locks_api()
            .get_locks_by_transaction_id(transaction_id)
            .unwrap(),
        lock_ids.to_vec()
    );
}

fn build_dry_run_transaction() -> Transaction {
    Transaction::builder_localnet(Epoch(100))
        .with_dry_run(true)
        .allocate_component_address("component")
        .put_last_instruction_output_on_workspace("bucket")
        .call_method("component", "new", args!["bucket"])
        .build_and_seal(&RistrettoSecretKey::from(1u64))
}

fn assert_not_stored(test: &TestWithNetwork<CannedTransactionResultInterface>, transaction_id: TransactionId) {
    let stored = test.store().with_read_tx(|tx| tx.transactions_get(transaction_id));
    assert!(
        stored.as_ref().is_err_and(|e| e.is_not_found_error()),
        "dry run {transaction_id} left a stored transaction: {stored:?}"
    );
}

#[tokio::test]
async fn a_dry_run_returns_its_result_without_storing_the_transaction() {
    let transaction = build_dry_run_transaction();
    let transaction_id = transaction.calculate_id();
    let fee_receipt = underpaid_fee_receipt();
    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::new(finalized_result(
        transaction_id,
        fee_receipt.clone(),
    )));

    let dry_run = test
        .sdk()
        .transaction_api()
        .submit_dry_run_transaction(transaction)
        .await
        .unwrap();

    assert_eq!(dry_run.id, transaction_id);
    assert_eq!(dry_run.status, TransactionStatus::DryRun);
    assert!(dry_run.is_dry_run);
    assert!(dry_run.finalize.is_some());
    assert_eq!(dry_run.final_fee, Some(fee_receipt.required_fees()));
    assert_eq!(dry_run.execution_time, Some(Duration::from_secs(1)));
    assert_not_stored(&test, transaction_id);
}

#[tokio::test]
async fn a_rejected_dry_run_does_not_store_the_transaction() {
    let transaction = build_dry_run_transaction();
    let transaction_id = transaction.calculate_id();
    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::new(TransactionQueryResult {
        transaction_id,
        result: TransactionFinalizedResult::Rejected {
            details: "too big".to_string(),
            rejected_time: now(),
        },
        receipt: None,
        encoded_execution_result: None,
    }));

    let err = test
        .sdk()
        .transaction_api()
        .submit_dry_run_transaction(transaction)
        .await
        .unwrap_err();

    assert!(err.to_string().contains("too big"), "{err}");
    assert_not_stored(&test, transaction_id);
}

#[tokio::test]
async fn commit_diff_preserves_downed_resource_metadata_vault_parent_and_fee_pool_token() {
    use tari_engine_types::{
        ValidatorFeePool,
        resource::Resource,
        resource_container::ResourceContainer,
        substate::{Substate, SubstateValue},
        vault::Vault,
    };
    use tari_ootle_common_types::VersionedSubstateIdRef;
    use tari_template_lib::types::{
        ComponentAddress,
        Metadata,
        ObjectKey,
        ResourceAddress,
        ResourceType,
        SubstateOwnerRule,
        TemplateAddress,
        ValidatorFeePoolAddress,
        VaultId,
        access_rules::ResourceAccessRules,
        constants::TARI_TOKEN,
    };

    let transaction = build_transaction();
    let transaction_id = transaction.calculate_id();

    let parent_component = SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([0x41; 32])));
    let child_vault_id = VaultId::new(ObjectKey::from_array([0x42; 32]));
    let child_vault = SubstateId::Vault(child_vault_id);
    let custom_resource_addr = ResourceAddress::new(ObjectKey::from_array([0x43; 32]));
    let custom_resource = SubstateId::Resource(custom_resource_addr);
    let fee_pool = SubstateId::ValidatorFeePool(ValidatorFeePoolAddress::from_array([0x44; 32]));
    let template_addr = TemplateAddress::from_array([0x45; 32]);

    let mut diff = SubstateDiff::new();
    diff.down(custom_resource.clone(), SubstateVersion::new(0));
    diff.down(child_vault.clone(), SubstateVersion::new(0));
    diff.up(
        custom_resource.clone(),
        Substate::new(
            1,
            SubstateValue::Resource(Box::new(Resource::new(
                ResourceType::Fungible,
                SubstateOwnerRule::None,
                ResourceAccessRules::new(),
                Metadata::new(),
                None,
                None,
                0,
                true,
            ))),
        ),
    );
    diff.up(
        child_vault.clone(),
        Substate::new(
            1,
            Vault::new(ResourceContainer::public_fungible(custom_resource_addr, 500u64)),
        ),
    );
    diff.up(
        fee_pool.clone(),
        Substate::new(
            0,
            SubstateValue::ValidatorFeePool(ValidatorFeePool::new([0x44; 32].into(), 100)),
        ),
    );

    let mut query_result = finalized_result(transaction_id, FeeReceipt::default());
    if let TransactionFinalizedResult::Finalized {
        execution_result: Some(exec),
        ..
    } = &mut query_result.result
    {
        exec.finalize.result = TransactionResult::Accept(diff);
    }

    let test = TestWithNetwork::with_network(CannedTransactionResultInterface::new(query_result));
    test.store()
        .with_write_tx(|tx| {
            tx.substates_upsert_root(
                VersionedSubstateIdRef::new(&custom_resource, SubstateVersion::new(0)),
                [parent_component.clone()].into_iter().collect(),
                Some("custom_mod".to_string()),
                Some(template_addr),
            )?;
            tx.substates_upsert_child(
                &parent_component,
                VersionedSubstateIdRef::new(&child_vault, SubstateVersion::new(0)),
                [custom_resource.clone()].into_iter().collect(),
            )?;
            tx.transactions_insert(&transaction, None, &[Test::test_account_address()], false)
        })
        .unwrap();

    test.sdk()
        .transaction_api()
        .check_and_store_finalized_transaction(transaction_id)
        .await
        .unwrap()
        .unwrap();

    let updated_resource = test.sdk().substate_api().get_substate(&custom_resource).unwrap();
    assert_eq!(updated_resource.substate_id.version(), SubstateVersion::new(1));
    assert_eq!(updated_resource.module_name.as_deref(), Some("custom_mod"));
    assert_eq!(updated_resource.template_address, Some(template_addr));
    assert_eq!(updated_resource.referenced_substates, vec![parent_component.clone()]);

    let updated_vault = test.sdk().substate_api().get_substate(&child_vault).unwrap();
    assert_eq!(updated_vault.substate_id.version(), SubstateVersion::new(1));
    assert_eq!(updated_vault.parent_address, Some(parent_component));
    assert_eq!(updated_vault.referenced_substates, vec![custom_resource]);

    let stored_fee_pool = test.sdk().substate_api().get_substate(&fee_pool).unwrap();
    assert_eq!(
        stored_fee_pool.referenced_substates,
        vec![SubstateId::Resource(TARI_TOKEN)]
    );
}
