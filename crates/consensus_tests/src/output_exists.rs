//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for transactions that cannot create one of their outputs, because the output exists or another
//! transaction creates it first.

use std::{collections::HashSet, iter};

use tari_common_types::types::PrivateKey;
use tari_consensus::hotstuff::HotStuffError;
use tari_consensus_types::Decision;
use tari_engine_types::{commit_result::AbortReason, substate::SubstateId};
use tari_ootle_common_types::{
    Epoch,
    InputDeclaration,
    NodeHeight,
    ShardGroup,
    SubstateLockType,
    SubstateVersion,
    VersionedSubstateId,
    VersionedSubstateIdRef,
    optional::Optional,
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    consensus_models::{Block, TransactionRecord},
};
use tari_ootle_transaction::{Transaction, TransactionId};

use crate::support::{
    ExecuteSpec,
    TEST_NUM_PRESHARDS,
    Test,
    TestAddress,
    TestVnDestination,
    build_substate_id_for_committee,
    helpers::random_substate_in_shard_group,
    logging::setup_logger,
};

const FULL_FEE: u64 = 38;
const FEE_INTENT_FEE: u64 = 19;

/// A transaction that writes `vault` and creates `outputs`, settling a fee-intent fallback.
fn creating(test: &Test, signer: u8, vault: &VersionedSubstateId, outputs: &[SubstateId]) -> TransactionRecord {
    creating_with_fee_intent(test, signer, vault, outputs, Some(FEE_INTENT_FEE))
}

/// A transaction that writes `vault` and creates `outputs`, settling a fee-intent fallback of `fee_intent_fee`.
fn creating_with_fee_intent(
    test: &Test,
    signer: u8,
    vault: &VersionedSubstateId,
    outputs: &[SubstateId],
    fee_intent_fee: Option<u64>,
) -> TransactionRecord {
    creating_from(test, signer, std::slice::from_ref(vault), outputs, fee_intent_fee)
}

/// A transaction that writes every one of `vaults` and creates `outputs`, settling a fee-intent fallback of
/// `fee_intent_fee`.
fn creating_from(
    test: &Test,
    signer: u8,
    vaults: &[VersionedSubstateId],
    outputs: &[SubstateId],
    fee_intent_fee: Option<u64>,
) -> TransactionRecord {
    let transaction = vaults
        .iter()
        .fold(Transaction::builder_localnet(Epoch(1)), |builder, vault| {
            builder.add_input(InputDeclaration::write_versioned(
                vault.substate_id().clone(),
                vault.version(),
            ))
        })
        .build_and_seal(&PrivateKey::from(u64::from(signer)));
    test.add_execution_at_destination(TestVnDestination::All, ExecuteSpec {
        transaction: transaction.clone(),
        decision: Decision::Commit,
        fee: FULL_FEE,
        input_locks: vaults
            .iter()
            .map(|vault| (vault.substate_id().clone(), SubstateLockType::Write))
            .collect(),
        new_outputs: outputs.to_vec(),
        validator_fee_withdrawals: vec![],
        calls_template: None,
        fee_intent_fallback_fee: fee_intent_fee,
    });
    TransactionRecord::new(transaction)
}

/// Waits until every validator has finalized every one of `transactions`.
async fn wait_for_finalized(test: &mut Test, transactions: &[&TransactionRecord]) {
    wait_for_finalized_at(test, TestVnDestination::All, transactions).await;
}

/// Waits until every validator at `destination` has finalized every one of `transactions`.
async fn wait_for_finalized_at(test: &mut Test, destination: TestVnDestination, transactions: &[&TransactionRecord]) {
    loop {
        test.on_block_committed().await;
        if transactions.iter().all(|transaction| {
            test.validators_iter()
                .filter(|validator| destination.is_for_vn(validator))
                .all(|validator| validator.has_finalized(transaction.id()))
        }) {
            return;
        }
        let leaf = test.get_validator(&TestAddress::new("1")).get_leaf_block();
        if leaf.height > NodeHeight(40) {
            test.dump_pool_info();
            panic!("Not all transactions finalized after {} blocks", leaf.height);
        }
    }
}

/// Whether every validator finalized `transaction` committing only its fee intent.
fn is_fee_intent_commit_everywhere(test: &Test, transaction: &TransactionRecord) -> bool {
    test.validators_iter().all(|validator| {
        let execution = validator.get_transaction_execution(transaction.id());
        let is_fee_only = execution.result().finalize.is_fee_only();
        if is_fee_only {
            assert_eq!(execution.transaction_fee(), FEE_INTENT_FEE, "{}", validator.address);
        }
        is_fee_only
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transaction_creating_an_output_that_exists_commits_its_fee_intent() {
    setup_logger();
    let mut test = Test::builder().add_committee(0, vec!["1", "2", "3", "4"]).start().await;
    let vaults = test.create_substates_on_vns(TestVnDestination::All, 2);
    let output = build_substate_id_for_committee(0, 1);
    let creator = creating(&test, 1, &vaults[0], std::slice::from_ref(&output));
    let duplicate = creating(&test, 2, &vaults[1], std::slice::from_ref(&output));

    test.send_transaction_to_destination(TestVnDestination::All, creator.clone())
        .await;
    test.start_epoch(Epoch(1)).await;
    wait_for_finalized(&mut test, &[&creator]).await;
    test.send_transaction_to_destination(TestVnDestination::All, duplicate.clone())
        .await;
    wait_for_finalized(&mut test, &[&duplicate]).await;
    test.stop();

    test.assert_all_validators_have_decision(creator.id(), Decision::Commit)
        .await;
    test.assert_all_validators_have_decision(duplicate.id(), Decision::Commit)
        .await;
    assert!(!is_fee_intent_commit_everywhere(&test, &creator));
    assert!(is_fee_intent_commit_everywhere(&test, &duplicate));

    test.assert_clean_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn of_two_transactions_creating_one_output_together_the_second_commits_its_fee_intent() {
    setup_logger();
    let mut test = Test::builder().add_committee(0, vec!["1", "2", "3", "4"]).start().await;
    let vaults = test.create_substates_on_vns(TestVnDestination::All, 2);
    let output = build_substate_id_for_committee(0, 1);
    let first = creating(&test, 1, &vaults[0], std::slice::from_ref(&output));
    let second = creating(&test, 2, &vaults[1], std::slice::from_ref(&output));

    test.send_transaction_to_destination(TestVnDestination::All, first.clone())
        .await;
    test.send_transaction_to_destination(TestVnDestination::All, second.clone())
        .await;
    test.start_epoch(Epoch(1)).await;
    wait_for_finalized(&mut test, &[&first, &second]).await;
    test.stop();

    test.assert_all_validators_have_decision(first.id(), Decision::Commit)
        .await;
    test.assert_all_validators_have_decision(second.id(), Decision::Commit)
        .await;
    // Whichever the block sequences first creates the output, and the other pays for its execution
    let fee_intent_only = [&first, &second]
        .into_iter()
        .filter(|transaction| is_fee_intent_commit_everywhere(&test, transaction))
        .count();
    assert_eq!(fee_intent_only, 1);

    test.assert_clean_shutdown().await;
}

/// A transaction the proposer leaves out of a block holds no locks in it, as on replicas, which never prepare it.
/// Otherwise its OUTPUT lock would make the proposer derive a fee-intent commit for a later transaction creating the
/// same output, which replicas commit in full, and the block would get no votes.
///
/// A proposal prepares its transactions in a random order, so each pair only exercises this when the oversized one
/// comes first. With enough pairs, nearly every proposal has one that does.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transaction_the_proposer_defers_does_not_take_its_outputs_from_the_block() {
    const NUM_PAIRS: usize = 10;
    setup_logger();
    let mut test = Test::builder()
        .add_committee(0, vec!["1", "2", "3", "4"])
        .with_max_block_shard_output_bytes(1_000)
        .start()
        .await;
    let vaults = test.create_substates_on_vns(TestVnDestination::All, 2 * NUM_PAIRS);
    let mut creators = Vec::with_capacity(NUM_PAIRS);
    for (pair, vaults) in vaults.chunks(2).enumerate() {
        let output = build_substate_id_for_committee(0, 1);
        // Every output of the oversized transaction lands in the shard of `output`, so its diff never fits the budget
        let shard = VersionedSubstateIdRef::new(&output, SubstateVersion::ZERO).to_shard(TEST_NUM_PRESHARDS);
        let oversized_outputs = iter::once(output.clone())
            .chain(
                iter::repeat_with(|| random_substate_in_shard_group(ShardGroup::new(shard, shard), TEST_NUM_PRESHARDS))
                    .take(40),
            )
            .collect::<Vec<_>>();
        let signer = u8::try_from(2 * pair + 1).unwrap();
        let oversized = creating(&test, signer, &vaults[0], &oversized_outputs);
        let creator = creating(&test, signer + 1, &vaults[1], std::slice::from_ref(&output));
        test.send_transaction_to_destination(TestVnDestination::All, oversized)
            .await;
        test.send_transaction_to_destination(TestVnDestination::All, creator.clone())
            .await;
        creators.push(creator);
    }

    test.start_epoch(Epoch(1)).await;
    wait_for_finalized(&mut test, &creators.iter().collect::<Vec<_>>()).await;
    test.stop();

    for creator in &creators {
        for validator in test.validators_iter() {
            let execution = validator.get_transaction_execution(creator.id());
            assert!(
                execution.result().finalize.is_full_accept(),
                "{}: {}",
                validator.address,
                execution.result().finalize.result
            );
        }
    }

    test.assert_clean_shutdown().await;
}

/// A shard group that finds a transaction's output already exists commits its fee intent, and so does every other
/// shard group involved, including one that accepted the full transaction first.
async fn assert_a_multishard_collision_commits_the_fee_intent_everywhere(colliding_committee: u32) {
    setup_logger();
    let mut test = Test::builder()
        .add_committee(0, vec!["1", "2"])
        .add_committee(1, vec!["3", "4"])
        .start()
        .await;
    let creator_vault = test
        .create_substates_on_vns(TestVnDestination::Committee(colliding_committee), 1)
        .remove(0);
    let vault = test
        .create_substates_on_vns(TestVnDestination::Committee(0), 1)
        .remove(0);
    let output = test.build_outputs_for_committee(colliding_committee, 1).remove(0);
    let fresh_output = test.build_outputs_for_committee(1, 1).remove(0);
    let creator = creating(&test, 1, &creator_vault, std::slice::from_ref(&output));
    let duplicate = creating(&test, 2, &vault, &[output, fresh_output]);

    test.send_transaction_to_destination(TestVnDestination::All, creator.clone())
        .await;
    test.start_epoch(Epoch(1)).await;
    wait_for_finalized_at(&mut test, TestVnDestination::Committee(colliding_committee), &[
        &creator,
    ])
    .await;
    test.send_transaction_to_destination(TestVnDestination::All, duplicate.clone())
        .await;
    wait_for_finalized(&mut test, &[&duplicate]).await;
    test.stop();

    test.assert_all_validators_have_decision(duplicate.id(), Decision::Commit)
        .await;
    assert!(is_fee_intent_commit_everywhere(&test, &duplicate));
    let reject_reasons = test
        .validators_iter()
        .map(|validator| {
            let execution = validator.get_transaction_execution(duplicate.id());
            execution.result().finalize.result.any_reject().map(ToString::to_string)
        })
        .collect::<HashSet<_>>();
    assert_eq!(reject_reasons.len(), 1, "{reject_reasons:?}");

    test.assert_clean_shutdown().await;
}

/// The output-only shard group finds the collision when it prepares, after the input shard group prepared the full
/// transaction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_output_only_shard_group_creating_an_output_that_exists_commits_the_fee_intent_everywhere() {
    assert_a_multishard_collision_commits_the_fee_intent_everywhere(1).await;
}

/// The input shard group finds the collision at LocalAccept, which the output-only shard group may already have sent
/// for the full transaction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_input_shard_group_creating_an_output_that_exists_commits_the_fee_intent_everywhere() {
    assert_a_multishard_collision_commits_the_fee_intent_everywhere(0).await;
}

/// The input shard group finds at LocalAccept that the output it creates exists, and with no fee intent to commit it
/// aborts. Its LocalAccept carries no outputs for the output-only shard group, which still applies the abort.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_input_shard_group_aborting_at_local_accept_aborts_the_output_only_shard_group() {
    setup_logger();
    let mut test = Test::builder()
        .add_committee(0, vec!["1", "2"])
        .add_committee(1, vec!["3", "4"])
        .start()
        .await;
    let vaults = test.create_substates_on_vns(TestVnDestination::Committee(0), 2);
    let output = test.build_outputs_for_committee(0, 1).remove(0);
    let foreign_output = test.build_outputs_for_committee(1, 1).remove(0);
    let creator = creating(&test, 1, &vaults[0], std::slice::from_ref(&output));
    let duplicate = creating_with_fee_intent(&test, 2, &vaults[1], &[output, foreign_output], None);

    test.send_transaction_to_destination(TestVnDestination::All, creator.clone())
        .await;
    test.start_epoch(Epoch(1)).await;
    wait_for_finalized_at(&mut test, TestVnDestination::Committee(0), &[&creator]).await;
    test.send_transaction_to_destination(TestVnDestination::All, duplicate.clone())
        .await;
    wait_for_finalized(&mut test, &[&duplicate]).await;
    test.stop();

    test.assert_all_validators_have_decision(duplicate.id(), Decision::Abort(AbortReason::LockOutputsFailed))
        .await;
    test.assert_all_validators_did_not_commit(duplicate.id());

    test.assert_clean_shutdown().await;
}

/// The number of shard groups in the evidence, and the leader fee, of the command with which the validator at
/// `address` committed `transaction_id`.
fn committed_involvement(test: &Test, address: &'static str, transaction_id: &TransactionId) -> (usize, u64) {
    test.get_validator(&TestAddress::new(address))
        .state_store
        .with_read_tx(|tx| {
            let leaf = tx.leaf_block_get(Epoch(1))?;
            let mut block = Some(Block::get(tx, &leaf.block_id)?);
            while let Some(current) = block {
                let committing = current
                    .commands()
                    .iter()
                    .filter_map(|command| command.committing())
                    .find(|atom| atom.id() == transaction_id);
                if let Some(atom) = committing {
                    let num_shard_groups = atom.evidence().map_or(1, |evidence| evidence.num_shard_groups());
                    let leader_fee = atom.leader_fee().map_or(0, |leader_fee| leader_fee.fee());
                    return Ok::<_, HotStuffError>((num_shard_groups, leader_fee));
                }
                block = current.get_parent(tx).optional()?;
            }
            panic!("{address} committed no command for {transaction_id}");
        })
        .unwrap()
}

/// A transaction with inputs in `input_committees` creates one output in every committee, and the outputs in
/// `colliding_committees` exist before it is sent. Every shard group commits its fee intent and counts every committee
/// as involved, so the leaders together take no more than the fee intent pays.
async fn assert_colliding_outputs_involve_every_shard_group(
    num_committees: u32,
    input_committees: &[u32],
    colliding_committees: &[u32],
) {
    const ADDRESSES: [&str; 4] = ["1", "2", "3", "4"];
    setup_logger();
    let addresses = &ADDRESSES[..num_committees as usize];
    let mut builder = Test::builder();
    for (committee, address) in (0..num_committees).zip(addresses.iter().copied()) {
        builder = builder.add_committee(committee, vec![address]);
    }
    let mut test = builder.start().await;

    let mut creators = Vec::with_capacity(colliding_committees.len());
    let mut outputs = Vec::with_capacity(num_committees as usize);
    for committee in 0..num_committees {
        let output = test.build_outputs_for_committee(committee, 1).remove(0);
        if colliding_committees.contains(&committee) {
            let creator_vault = test
                .create_substates_on_vns(TestVnDestination::Committee(committee), 1)
                .remove(0);
            let signer = u8::try_from(100 + committee).unwrap();
            let creator = creating(&test, signer, &creator_vault, std::slice::from_ref(&output));
            creators.push((committee, creator));
        }
        outputs.push(output);
    }
    let vaults = input_committees
        .iter()
        .map(|committee| {
            test.create_substates_on_vns(TestVnDestination::Committee(*committee), 1)
                .remove(0)
        })
        .collect::<Vec<_>>();
    let duplicate = creating_from(&test, 1, &vaults, &outputs, Some(FEE_INTENT_FEE));

    for (_, creator) in &creators {
        test.send_transaction_to_destination(TestVnDestination::All, creator.clone())
            .await;
    }
    test.start_epoch(Epoch(1)).await;
    for (committee, creator) in &creators {
        wait_for_finalized_at(&mut test, TestVnDestination::Committee(*committee), &[creator]).await;
    }
    test.send_transaction_to_destination(TestVnDestination::All, duplicate.clone())
        .await;
    wait_for_finalized(&mut test, &[&duplicate]).await;
    test.stop();

    test.assert_all_validators_have_decision(duplicate.id(), Decision::Commit)
        .await;
    assert!(is_fee_intent_commit_everywhere(&test, &duplicate));
    let mut total_leader_fee = 0;
    for address in addresses.iter().copied() {
        let (num_shard_groups, leader_fee) = committed_involvement(&test, address, duplicate.id());
        assert_eq!(num_shard_groups, num_committees as usize, "{address}");
        total_leader_fee += leader_fee;
    }
    assert!(
        total_leader_fee <= FEE_INTENT_FEE,
        "leaders took {total_leader_fee} of a fee of {FEE_INTENT_FEE}"
    );

    test.assert_clean_shutdown().await;
}

/// One output-only shard group finds its output exists, and another creates its output.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_colliding_output_only_shard_group_involves_every_other_output_shard_group() {
    assert_colliding_outputs_involve_every_shard_group(3, &[0], &[1]).await;
}

/// Every output-only shard group finds its output exists.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn output_only_shard_groups_that_all_collide_share_one_fee() {
    assert_colliding_outputs_involve_every_shard_group(4, &[0], &[1, 2, 3]).await;
}

/// One of two input shard groups finds its output exists at LocalAccept, and an output-only shard group creates its
/// output.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_colliding_input_shard_group_involves_the_output_only_shard_group() {
    assert_colliding_outputs_involve_every_shard_group(3, &[0, 1], &[1]).await;
}
