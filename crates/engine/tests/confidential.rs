//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_engine_types::{
    crypto::{ElgamalVerifiableBalance, OutputBody, ValueLookupTable},
    resource_container::ResourceError,
    substate::SubstateId,
};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::{
    models::Account,
    types::{
        Amount,
        ComponentAddress,
        confidential::ConfidentialOutputStatement,
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    },
};
use tari_template_test_tooling::{
    TemplateTest,
    support::{
        GenerateValueLookup,
        assert_error::assert_reject_reason,
        confidential::{
            generate_confidential_output_statement,
            generate_confidential_proof_with_view_key,
            generate_withdraw_proof,
            generate_withdraw_proof_with_inputs,
            generate_withdraw_proof_with_view_key,
        },
    },
};
use tari_transaction_manifest::ManifestValue;
use tari_utilities::ByteArray;

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

fn setup(
    initial_supply: ConfidentialOutputStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> (TemplateTest, ComponentAddress, SubstateId) {
    let mut template_test = TemplateTest::new(CRATE_PATH, vec![
        "tests/templates/confidential/faucet",
        "tests/templates/confidential/utilities",
    ]);

    let faucet: ComponentAddress = view_key
        .map(|vk| {
            let vk = RistrettoPublicKeyBytes::from_bytes(vk.as_bytes()).unwrap();
            template_test.call_function(
                "ConfidentialFaucet",
                "mint_with_view_key",
                args![initial_supply, vk],
                vec![],
            )
        })
        .unwrap_or_else(|| template_test.call_function("ConfidentialFaucet", "mint", args![initial_supply], vec![]));

    let resx = template_test.get_previous_output_address(SubstateType::Resource);

    (template_test, faucet, resx)
}

#[test]
fn mint_initial_commitment() {
    let (confidential_proof, _mask, _change) = generate_confidential_output_statement(100, None);
    let (test, _faucet, faucet_resx) = setup(confidential_proof, None);

    let resource = test
        .read_only_state_store()
        .get_resource(&faucet_resx.as_resource_address().unwrap())
        .unwrap();
    // TODO: confidential total_supply tracking only tracks revealed funds
    assert_eq!(resource.total_supply(), Some(Amount::from(0u64)));
}

#[test]
fn mint_more_later() {
    let (confidential_proof, _mask, _change) = generate_confidential_output_statement(0, None);
    let (mut template_test, faucet, _faucet_resx) = setup(confidential_proof, None);

    let (confidential_proof, mask, _change) = generate_confidential_output_statement(100, None);
    template_test.call_method::<()>(faucet, "mint_more", args![confidential_proof], vec![]);

    let (user_account, user_proof, user_key) = template_test.create_empty_account();

    let withdraw_proof = generate_withdraw_proof(&mask, 100, None, 0u64);
    template_test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "take_free_coins", args![withdraw_proof.proof])
            .put_last_instruction_output_on_workspace("coins")
            .call_method(user_account, "deposit", args![Workspace("coins")])
            .build_and_seal(&user_key),
        vec![user_proof],
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn transfer_confidential_amounts_between_accounts() {
    let (confidential_proof, faucet_mask, _change) = generate_confidential_output_statement(100_000, None);
    let (mut template_test, faucet, faucet_resx) = setup(confidential_proof, None);

    // Create an account
    let (account1, owner1, _k) = template_test.create_funded_account();
    let (account2, _owner2, _k) = template_test.create_funded_account();

    // Create proof for transfer
    let proof = generate_withdraw_proof(&faucet_mask, 1000, Some(99_000), 0u64);

    // Transfer faucet funds into account 1
    let vars = [
        ("faucet", faucet.into()),
        ("account1", account1.into()),
        ("proof", ManifestValue::new_value(&proof.proof).unwrap()),
    ];
    let result = template_test
        .execute_and_commit_manifest(
            r#"
        let faucet = var!["faucet"];
        let account1 = var!["account1"];
        let proof = var!["proof"];
        let coins = faucet.take_free_coins(proof);
        account1.deposit(coins);
    "#,
            vars,
            vec![],
        )
        .unwrap();

    let diff = result.finalize.result.expect("Failed to execute manifest");
    assert_eq!(diff.up_iter().filter(|(addr, _)| *addr == account1).count(), 1);
    assert_eq!(diff.down_iter().filter(|(addr, _)| *addr == account1).count(), 1);
    // Faucet is not changed, only the faucet vault.
    assert_eq!(diff.up_iter().filter(|(addr, _)| *addr == faucet).count(), 0);
    assert_eq!(diff.down_iter().filter(|(addr, _)| *addr == faucet).count(), 0);
    assert_eq!(diff.up_iter().count(), 4);
    assert_eq!(diff.down_iter().count(), 2);

    let withdraw_proof = generate_withdraw_proof(&proof.output_mask, 100, Some(900), 0u64);
    let split_proof = generate_withdraw_proof(&withdraw_proof.output_mask, 20, Some(80), 0u64);

    let vars = [
        ("faucet_resx", faucet_resx.into()),
        ("account1", account1.into()),
        ("account2", account2.into()),
        (
            "withdraw_proof",
            ManifestValue::new_value(&withdraw_proof.proof).unwrap(),
        ),
        ("split_proof", ManifestValue::new_value(&split_proof.proof).unwrap()),
    ];
    let result = template_test
        .execute_and_commit_manifest(
            r#"
        let account1 = var!["account1"];
        let account2 = var!["account2"];

        let faucet_resx = var!["faucet_resx"];
        let withdraw_proof = var!["withdraw_proof"];
        let coins1 = account1.withdraw_confidential(faucet_resx, withdraw_proof);

        let split_proof = var!["split_proof"];
        let coins2 = ConfidentialUtilities::take_from_bucket(coins1, split_proof);

        account1.deposit(coins1);
        account2.deposit(coins2);
    "#,
            vars,
            vec![owner1],
        )
        .unwrap();
    let diff = result.finalize.result.expect("Failed to execute manifest");
    assert_eq!(diff.up_iter().filter(|(addr, _)| *addr == account1).count(), 0);
    assert_eq!(diff.down_iter().filter(|(addr, _)| *addr == account1).count(), 0);
    assert_eq!(diff.up_iter().filter(|(addr, _)| *addr == account2).count(), 1);
    assert_eq!(diff.down_iter().filter(|(addr, _)| *addr == account2).count(), 1);
    assert_eq!(diff.up_iter().count(), 4);
    assert_eq!(diff.down_iter().count(), 2);
}

#[test]
fn transfer_confidential_fails_with_invalid_balance() {
    let (confidential_proof, faucet_mask, _change) = generate_confidential_output_statement(100_000, None);
    let (mut template_test, faucet, _faucet_resx) = setup(confidential_proof, None);

    // Create an account
    let (account1, _owner1, _k) = template_test.create_funded_account();

    // Create proof for transfer
    let proof = generate_withdraw_proof(&faucet_mask, 1001, Some(99_000), 0u64);

    // Transfer faucet funds into account 1
    let vars = [
        ("faucet", faucet.into()),
        ("account1", account1.into()),
        ("proof", ManifestValue::new_value(&proof.proof).unwrap()),
    ];
    let _err = template_test
        .execute_and_commit_manifest(
            r#"
        let faucet = var!["faucet"];
        let account1 = var!["account1"];
        let proof = var!["proof"];
        let coins = faucet.take_free_coins(proof);
        account1.deposit(coins);
    "#,
            vars,
            vec![],
        )
        .unwrap_err();
}

#[test]
fn reveal_confidential_and_transfer() {
    let (confidential_proof, faucet_mask, _change) = generate_confidential_output_statement(100_000, None);
    let (mut test, faucet, faucet_resx) = setup(confidential_proof, None);

    // Create an account
    let (account1, owner1, _k) = test.create_funded_account();
    let (account2, owner2, _k) = test.create_funded_account();

    // Create proof for transfer

    let proof = generate_withdraw_proof(&faucet_mask, 1000, Some(99_000), 0u64);
    // Reveal 90 tokens and 10 confidentially
    let reveal_proof = generate_withdraw_proof(&proof.output_mask, 10, Some(900), 90u64);
    // Then reveal the rest
    let reveal_bucket_proof = generate_withdraw_proof(&reveal_proof.output_mask, 0, None, 10u64);

    let faucet_resx = faucet_resx.as_resource_address().unwrap();
    // Transfer faucet funds into account 1
    let vars = [
        ("faucet", faucet.into()),
        ("resource", faucet_resx.into()),
        ("account1", account1.into()),
        ("account2", account2.into()),
        ("proof", ManifestValue::new_value(&proof.proof).unwrap()),
        ("reveal_proof", ManifestValue::new_value(&reveal_proof.proof).unwrap()),
        (
            "reveal_bucket_proof",
            ManifestValue::new_value(&reveal_bucket_proof.proof).unwrap(),
        ),
    ];
    let _result = test
        .execute_and_commit_manifest(
            r#"
        let faucet = var!["faucet"];
        let account1 = var!["account1"];
        let account2 = var!["account2"];
        let proof = var!["proof"];
        let reveal_proof = var!["reveal_proof"];
        let reveal_bucket_proof = var!["reveal_bucket_proof"];
        let resource = var!["resource"];

        // Take confidential coins from faucet and deposit into account 1
        let coins = faucet.take_free_coins(proof);
        account1.deposit(coins);

        // Reveal 90 tokens and 10 confidentially and deposit both funds into account 2
        let revealed_funds = account1.withdraw_confidential(resource, reveal_proof);
        let revealed_rest_funds = ConfidentialUtilities::take_from_bucket(revealed_funds, reveal_bucket_proof);
        let joined = ConfidentialUtilities::join_buckets(revealed_funds, revealed_rest_funds);
        account2.deposit(joined);

        // Account2 can withdraw revealed funds by amount
        let small_amt = account2.withdraw(resource, 10);
        account1.deposit(small_amt);

        account1.balance(resource);
        account2.balance(resource);
    "#,
            vars,
            vec![owner1, owner2],
        )
        .unwrap();

    let acc1 = test.read_only_state_store().get_component(account1).unwrap();
    let acc1 = Account::from_value(acc1.state()).unwrap();
    let vault1 = acc1.get_vault_by_resource(&faucet_resx).unwrap();
    let vault1 = test.read_only_state_store().get_vault(&vault1.vault_id()).unwrap();
    assert_eq!(vault1.balance(), 10);

    let acc2 = test.read_only_state_store().get_component(account2).unwrap();
    let acc2 = Account::from_value(acc2.state()).unwrap();
    let vault2 = acc2.get_vault_by_resource(&faucet_resx).unwrap();
    let vault2 = test.read_only_state_store().get_vault(&vault2.vault_id()).unwrap();
    assert_eq!(vault2.balance(), 90);
}

#[test]
fn attempt_to_reveal_with_unbalanced_proof() {
    let (confidential_proof, faucet_mask, _change) = generate_confidential_output_statement(100_000, None);
    let (mut template_test, faucet, faucet_resx) = setup(confidential_proof, None);

    // Create an account
    let (account1, owner1, _k) = template_test.create_funded_account();
    let (account2, _owner2, _k) = template_test.create_funded_account();

    // Create proof for transfer

    let proof = generate_withdraw_proof(&faucet_mask, 1000, Some(99_000), 0u64);
    // Attempt to reveal more than input - change
    let reveal_proof = generate_withdraw_proof(&proof.output_mask, 0, Some(900), 110u64);

    // Transfer faucet funds into account 1
    let vars = [
        ("faucet", faucet.into()),
        ("resource", faucet_resx.into()),
        ("account1", account1.into()),
        ("account2", account2.into()),
        ("proof", ManifestValue::new_value(&proof.proof).unwrap()),
        ("reveal_proof", ManifestValue::new_value(&reveal_proof.proof).unwrap()),
    ];

    // TODO: Propagate error messages from runtime
    let _err = template_test
        .execute_and_commit_manifest(
            r#"
        let faucet = var!["faucet"];
        let account1 = var!["account1"];
        let account2 = var!["account2"];
        let proof = var!["proof"];
        let reveal_proof = var!["reveal_proof"];
        let resource = var!["resource"];

        // Take confidential coins from faucet and deposit into account 1
        let coins = faucet.take_free_coins(proof);
        account1.deposit(coins);

        // Reveal 100 tokens and deposit revealed funds into account 2
        let revealed_funds = account1.withdraw_confidential(resource, reveal_proof);
        account2.deposit(revealed_funds);

        account1.balance(resource);
        account2.balance(resource);
    "#,
            vars,
            vec![owner1],
        )
        .unwrap_err();
}

#[test]
fn multi_commitment_join() {
    let (confidential_proof, faucet_mask, _change) = generate_confidential_output_statement(100_000, None);
    let (mut template_test, faucet, faucet_resx) = setup(confidential_proof, None);

    // Create an account
    let (account1, owner1, _k) = template_test.create_funded_account();

    // Create proof for transfer

    let withdraw_proof1 = generate_withdraw_proof(&faucet_mask, 1000, Some(99_000), 0u64);
    let withdraw_proof2 =
        generate_withdraw_proof(withdraw_proof1.change_mask.as_ref().unwrap(), 1000, Some(98_000), 0u64);
    let join_proof = generate_withdraw_proof_with_inputs(
        &[(withdraw_proof1.output_mask, 1000), (withdraw_proof2.output_mask, 1000)],
        0u64,
        2000,
        None,
        0,
    );

    // Transfer faucet funds into account 1
    let vars = [
        ("faucet", faucet.into()),
        ("resource", faucet_resx.into()),
        ("account1", account1.into()),
        (
            "withdraw_proof1",
            ManifestValue::new_value(&withdraw_proof1.proof).unwrap(),
        ),
        (
            "withdraw_proof2",
            ManifestValue::new_value(&withdraw_proof2.proof).unwrap(),
        ),
        ("join_proof", ManifestValue::new_value(&join_proof.proof).unwrap()),
    ];
    let result = template_test
        .execute_and_commit_manifest(
            r#"
        let faucet = var!["faucet"];
        let account1 = var!["account1"];
        let withdraw_proof1 = var!["withdraw_proof1"];
        let withdraw_proof2 = var!["withdraw_proof2"];
        let join_proof = var!["join_proof"];
        let resource = var!["resource"];

        // Take confidential coins from faucet and deposit into account
        let coins = faucet.take_free_coins(withdraw_proof1);
        account1.deposit(coins);
        account1.confidential_commitment_count(resource);

        let coins = faucet.take_free_coins(withdraw_proof2);
        account1.deposit(coins);

        // Should contain 2 commitments
        account1.confidential_commitment_count(resource);

        /// Join the two commitments valued at 1000 each
        account1.join_confidential(resource, join_proof);

        // Now we have one commitment valued at 2000
        account1.confidential_commitment_count(resource);
    "#,
            vars,
            vec![owner1],
        )
        .unwrap();

    assert_eq!(result.finalize.execution_results[3].decode::<u32>().unwrap(), 1);
    assert_eq!(result.finalize.execution_results[7].decode::<u32>().unwrap(), 2);
    assert_eq!(result.finalize.execution_results[9].decode::<u32>().unwrap(), 1);
}

#[test]
fn mint_and_transfer_revealed() {
    let (confidential_proof, _mask, _change) = generate_confidential_output_statement(100, None);
    let (mut test, faucet, faucet_resx) = setup(confidential_proof, None);

    let faucet_resx = faucet_resx.as_resource_address().unwrap();

    let (user_account, _, _) = test.create_empty_account();

    test.call_method::<()>(faucet, "mint_revealed", args![123], vec![]);
    let balance: Amount = test.call_method(faucet, "vault_balance", args![], vec![]);
    assert_eq!(balance, Amount::from(123u64));

    // Convert 100 revealed funds to confidential and the remaining 23 to revealed
    let withdraw = generate_withdraw_proof_with_inputs(&[], 123u64, 100, None, 23);

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "take_free_coins", args![withdraw.proof])
            .put_last_instruction_output_on_workspace("b")
            .call_method(user_account, "deposit", args![Workspace("b")])
            .call_method(user_account, "balance", args![faucet_resx])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    // The account should have a revealed balance of 23 revealed funds
    let account_balance = result.finalize.execution_results[3].decode::<Amount>().unwrap();
    assert_eq!(account_balance, 23);
}

#[test]
fn mint_revealed_with_invalid_proof() {
    let (confidential_proof, _mask, _change) = generate_confidential_output_statement(100, None);
    let (mut test, faucet, _faucet_resx) = setup(confidential_proof, None);

    let reason = test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(faucet, "mint_revealed_with_bad_range_proof", args![123])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    assert_reject_reason(reason, ResourceError::InvalidConfidentialProof {
        details: String::new(),
    });
}

pub fn try_brute_force_confidential_balance<I, TValueLookup>(
    utxos: &BTreeMap<PedersenCommitmentBytes, OutputBody>,
    secret_view_key: &RistrettoSecretKey,
    value_range: I,
    value_lookup: &mut TValueLookup,
) -> Result<Option<u64>, TValueLookup::Error>
where
    I: IntoIterator<Item = u64> + Clone,
    TValueLookup: ValueLookupTable,
{
    let decompressed_viewable_balances = utxos
        .values()
        .filter_map(|utxo| utxo.viewable_balance.as_ref().map(|vb| vb.try_into().unwrap()))
        .collect::<Vec<_>>();

    let balances = ElgamalVerifiableBalance::batched_brute_force(
        secret_view_key,
        value_range,
        value_lookup,
        &decompressed_viewable_balances,
    )?;

    // If any of the commitments cannot be brute forced, then we return None
    Ok(balances.into_iter().sum())
}

#[test]
fn mint_with_view_key() {
    let (view_key_secret, ref view_key) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let (confidential_proof, _mask, _change) = generate_confidential_proof_with_view_key(123, None, view_key);
    let (mut test, faucet, _faucet_resx) = setup(confidential_proof, Some(view_key));
    let faucet_entity_id = faucet.entity_id();

    let (confidential_proof, mask, _change) = generate_confidential_proof_with_view_key(100, None, view_key);
    test.call_method::<()>(faucet, "mint_more", args![confidential_proof], vec![]);

    let (user_account, user_proof, user_key) = test.create_empty_account();
    let user_account_entity_id = user_account.entity_id();

    let withdraw_proof = generate_withdraw_proof_with_view_key(&mask, 100, 55, Some(100 - 55), 0u64, view_key);
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(faucet, "take_free_coins", args![withdraw_proof.proof])
            .put_last_instruction_output_on_workspace("coins")
            .call_method(user_account, "deposit", args![Workspace("coins")])
            .build_and_seal(&user_key),
        vec![user_proof],
    );

    let diff = result.finalize.result.any_accept().unwrap();
    let faucet_vault = diff
        .up_iter()
        .find(|(addr, _)| addr.is_vault() && addr.as_vault_id().unwrap().entity_id() == faucet_entity_id)
        .map(|(_, vault)| vault.substate_value().as_vault().unwrap())
        .unwrap();

    let total_balance = try_brute_force_confidential_balance(
        faucet_vault.get_confidential_commitments().unwrap(),
        &view_key_secret,
        0..=200,
        &mut GenerateValueLookup,
    )
    .unwrap();
    assert_eq!(total_balance, Some(223 - 55));

    let user_vault = diff
        .up_iter()
        .find(|(addr, _)| addr.is_vault() && addr.as_vault_id().unwrap().entity_id() == user_account_entity_id)
        .map(|(_, vault)| vault.substate_value().as_vault().unwrap())
        .unwrap();

    let total_balance = try_brute_force_confidential_balance(
        user_vault.get_confidential_commitments().unwrap(),
        &view_key_secret,
        0..=200,
        &mut GenerateValueLookup,
    )
    .unwrap();
    assert_eq!(total_balance, Some(55));
}
