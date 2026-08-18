//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{iter, time::Duration};

use tari_common_types::types::PrivateKey;
use tari_consensus_types::Decision;
use tari_engine_types::{
    Epoch,
    ValidatorFeePool,
    ValidatorFeeWithdrawal,
    commit_result::{ExecuteResult, FinalizeResult, RejectReason, TransactionResult},
    component::{Component, ComponentBody, ComponentHeader},
    fees::{FeeBreakdown, FeeReceipt, FeeSource},
    published_template::PublishedTemplate,
    substate::{Substate, SubstateDiff, SubstateId},
    transaction_receipt::{FinalizeOutcome, TransactionReceipt},
};
use tari_ootle_common_types::{LockIntent, SubstateRequirement};
use tari_ootle_storage::consensus_models::{TransactionRecord, VersionedSubstateIdLockIntent};
use tari_ootle_transaction::{Transaction, TransactionIntent, args};
use tari_template_lib_types::{SubstateOwnerRule, TransactionReceiptAddress};

use crate::support::{TEST_NUM_PRESHARDS, committee_number_to_shard_group, helpers::random_substate_in_shard_group};

/// WASM metering points attributed to every fabricated test execution — a typical "normal"
/// transaction. Sized so that even a block filled to `max_commands_in_block` stays within the
/// default `max_block_execution_points` budget: normal test transactions must never hit the budget.
/// `TestBuilder::start` asserts this invariant against the final test config.
pub const TEST_WASM_EXECUTION_POINTS: u64 = 5_000_000;

pub fn build_transaction_from(tx: Transaction) -> TransactionRecord {
    TransactionRecord::new(tx)
}

/// Fabricates a fully-paid fee receipt for a test execution. `fee` is the pre-burn execution fee; a 5% exhaust
/// burn is charged on top, mirroring the shape the real executor produces.
fn create_test_fee_receipt(fee: u64) -> FeeReceipt {
    let exhaust_burn = fee / 20;
    let mut cost_breakdown = FeeBreakdown::default();
    cost_breakdown.add(FeeSource::WasmExecution, fee);
    cost_breakdown.add(FeeSource::ExhaustBurn, exhaust_burn);
    FeeReceipt::builder()
        .with_total_fee_payment(fee + exhaust_burn)
        .with_total_fees_paid(fee + exhaust_burn)
        .with_total_fee_overcharge(0)
        .with_cost_breakdown(cost_breakdown)
        .build()
}

#[allow(clippy::too_many_lines)]
pub fn create_execution_result_for_transaction(
    transaction: &Transaction,
    decision: Decision,
    fee: u64,
    resolved_inputs: &[VersionedSubstateIdLockIntent],
    resulting_outputs: &[VersionedSubstateIdLockIntent],
    validator_fee_withdrawals: Vec<ValidatorFeeWithdrawal>,
) -> ExecuteResult {
    let result = if decision.is_commit() {
        let mut diff = SubstateDiff::new();
        for input in resolved_inputs.iter().filter(|input| input.lock_type().is_write()) {
            diff.down(
                input.versioned_substate_id().substate_id().clone(),
                input.versioned_substate_id().version(),
            );
        }
        for output in resulting_outputs {
            if output.substate_id().is_transaction_receipt() {
                continue;
            }

            match output.substate_id() {
                SubstateId::Component(_) => {
                    // Generate consistent state for the component by simply using the ID
                    let state = tari_bor::to_value(output.versioned_substate_id()).unwrap();
                    diff.up(
                        output.versioned_substate_id().substate_id().clone(),
                        Substate::new(output.versioned_substate_id().version(), Component {
                            header: ComponentHeader {
                                template_address: Default::default(),
                                owner_rule: SubstateOwnerRule::None,
                                access_rules: Default::default(),
                                entity_id: output
                                    .versioned_substate_id()
                                    .substate_id()
                                    .as_component_address()
                                    .unwrap()
                                    .entity_id(),
                            },
                            body: ComponentBody { state },
                        }),
                    );
                },
                SubstateId::Template(_) => {
                    let binary = transaction
                        .instructions()
                        .iter()
                        .find_map(|i| i.published_template_binary_index())
                        .map(|idx| transaction.blobs().get(idx).expect("blob not found"))
                        .expect("No publish template instruction found in transaction");
                    diff.up(
                        output.versioned_substate_id().substate_id().clone(),
                        Substate::new(output.versioned_substate_id().version(), PublishedTemplate {
                            template_name: "test".try_into().expect("valid name"),
                            author: *transaction.seal_signature().public_key(),
                            binary: binary.to_vec().try_into().expect("Template binary too large"),
                            at_epoch: 0,
                            metadata_hash: None,
                        }),
                    );
                },
                SubstateId::ValidatorFeePool(_) => {
                    diff.up(
                        output.versioned_substate_id().substate_id().clone(),
                        Substate::new(output.versioned_substate_id().version(), ValidatorFeePool {
                            // This does not matter in tests
                            claim_public_key: Default::default(),
                            amount: 100_000,
                        }),
                    );
                },
                _ => {
                    panic!(
                        "create_execution_result_for_transaction: Test harness only supports generating component, vn \
                         fee, and template outputs. Got {output}"
                    );
                },
            }
        }
        // We MUST create the transaction receipt
        diff.up(
            SubstateId::TransactionReceipt(TransactionReceiptAddress::from(transaction.calculate_id())),
            Substate::new(0, TransactionReceipt {
                outcome: FinalizeOutcome::Commit,
                diff_summary: Default::default(),
                fee_withdrawals: Default::default(),
                events: Default::default(),
                logs: Default::default(),
                fee_receipt: create_test_fee_receipt(fee),
                epoch: Epoch::zero(),
                intent_commitment: transaction.calculate_intent_commitment(),
            }),
        );

        diff.set_once_fee_withdrawals(validator_fee_withdrawals);

        TransactionResult::Accept(diff)
    } else {
        TransactionResult::Reject(RejectReason::ExecutionFailure(
            "Transaction was set to ABORT in test".to_string(),
        ))
    };

    ExecuteResult {
        finalize: FinalizeResult::new(
            transaction.calculate_id().into_array().into(),
            vec![],
            vec![],
            result,
            create_test_fee_receipt(fee),
        ),
        execution_time: Duration::from_secs(0),
        execute_epoch: None,
        wasm_execution_points: TEST_WASM_EXECUTION_POINTS,
        native_execution_points: 0,
    }
}

pub fn build_substate_id_for_committee(committee_no: u32, num_committees: u32) -> SubstateId {
    random_substates_ids_for_committee_generator(committee_no, num_committees)
        .next()
        .unwrap()
}

pub fn random_substates_ids_for_committee_generator(
    committee_no: u32,
    num_committees: u32,
) -> impl Iterator<Item = SubstateId> {
    iter::repeat_with(move || {
        random_substate_in_shard_group(
            committee_number_to_shard_group(TEST_NUM_PRESHARDS, committee_no, num_committees),
            TEST_NUM_PRESHARDS,
        )
    })
}

pub fn build_transaction(inputs: Vec<SubstateRequirement>) -> TransactionRecord {
    let k = PrivateKey::default();
    let tx = Transaction::builder_localnet(Epoch(1))
        .call_function(Default::default(), "foo", args![])
        .with_inputs(inputs)
        .build_and_seal(&k);
    TransactionRecord::new(tx)
}
