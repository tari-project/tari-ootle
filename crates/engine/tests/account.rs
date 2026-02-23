//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine::runtime::{ActionIdent, RuntimeError};
use tari_ootle_transaction::{Instruction, Transaction, args, call_args};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::types::{Amount, ComponentAddress, access_rules::ComponentAccessRules, constants::XTR, rule};
use tari_template_test_tooling::{
    TemplateTest,
    support::assert_error::{assert_access_denied_for_action, assert_reject_reason},
    xtr_faucet_component,
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn basic_faucet_transfer() {
    let mut template_test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());

    let (faucet_component, faucet_resource) = template_test.create_test_faucet_component(1_000_000_000_000u64);

    // Create sender and receiver accounts
    let (sender_address, sender_proof, _) = template_test.create_funded_account();
    let (receiver_address, _, _) = template_test.create_funded_account();

    let _result = template_test
        .build_and_execute(
            Transaction::builder_localnet()
                .call_method(faucet_component, "take_free_coins", args![])
                .put_last_instruction_output_on_workspace("free_coins")
                .call_method(sender_address, "deposit", args![Workspace("free_coins")]),
            vec![template_test.owner_proof()],
        )
        .unwrap_success();

    let result = template_test
        .build_and_execute(
            Transaction::builder_localnet()
                .call_method(sender_address, "withdraw", args![faucet_resource, 100])
                .put_last_instruction_output_on_workspace("foo_bucket")
                .call_method(receiver_address, "deposit", args![Workspace("foo_bucket")])
                .call_method(sender_address, "balance", args![faucet_resource])
                .call_method(receiver_address, "balance", args![faucet_resource]),
            // Sender proof needed to withdraw
            vec![sender_proof],
        )
        .unwrap_success();

    assert_eq!(
        result.finalize.execution_results[3].decode::<Amount>().unwrap(),
        Amount::from(900u64)
    );
    assert_eq!(
        result.finalize.execution_results[4].decode::<Amount>().unwrap(),
        Amount::from(100u64)
    );
}

#[test]
fn withdraw_from_account_prevented() {
    let mut template_test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());

    let faucet_template = template_test.get_template_address("TestFaucet");

    let initial_supply = Amount::from(1_000_000_000_000u64);
    let result = template_test
        .execute_and_commit(
            vec![Instruction::CallFunction {
                address: faucet_template,
                function: "mint".try_into().unwrap(),
                args: call_args![initial_supply],
            }],
            vec![template_test.owner_proof()],
        )
        .unwrap();
    let faucet_component: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();
    let faucet_resource = result
        .finalize
        .result
        .expect("Faucet mint failed")
        .up_iter()
        .find_map(|(addr, _)| addr.as_resource_address())
        .unwrap();

    // Create sender and receiver accounts
    let (source_account, _, _) = template_test.create_funded_account();

    let _result = template_test
        .execute_and_commit_manifest(
            r#"
                let source_account = var!["source_account"];
                let faucet_component = var!["faucet_component"];
                
                let free_coins = faucet_component.take_free_coins();
                source_account.deposit(free_coins);
            "#,
            [
                ("source_account", source_account.into()),
                ("faucet_component", faucet_component.into()),
            ],
            vec![],
        )
        .unwrap();

    let (dest_address, non_owning_token, non_owning_key) = template_test.create_funded_account();

    let reason = template_test.execute_expect_failure(
        Transaction::builder_localnet()
            .call_method(source_account, "withdraw", args![faucet_resource, 100])
            .put_last_instruction_output_on_workspace("stolen_coins")
            .call_method(source_account, "deposit", args![Workspace("stolen_coins")])
            .build_and_seal(&non_owning_key),
        // VNs provide the token that signed the transaction, which in this case is the non_owning_token
        vec![non_owning_token],
    );

    assert_access_denied_for_action(reason, ActionIdent::ComponentCallMethod {
        component_address: source_account,
        method: "withdraw".to_string(),
    });

    let result = template_test
        .execute_and_commit_manifest(
            r#"
                let dest_account = var!["dest_account"];
                let resource = var!["resource"];
                dest_account.balance(resource);
            "#,
            [
                ("dest_account", dest_address.into()),
                ("resource", faucet_resource.into()),
            ],
            // Nothing required for balance check at the moment
            vec![],
        )
        .unwrap();
    assert_eq!(
        result.finalize.execution_results[0].decode::<Amount>().unwrap(),
        Amount::zero()
    );
}

#[test]
fn attempt_to_overwrite_account() {
    let mut test = TemplateTest::new_builtin_only();

    // Create initial account with faucet funds
    let (source_account, source_account_proof, source_account_sk) = test.create_funded_account();

    let null: Option<()> = None;
    let overwriting_tx = test.execute_expect_failure(
        Transaction::builder_localnet()
            // Create component with the same ID
            // The create account instruction is idempotent, so we'll call the template directly to force an overwrite attempt
            .call_function(
                ACCOUNT_TEMPLATE_ADDRESS,
                "create",
                args![source_account_proof, null, null, null],
            )
            // Signed by source account so that it can pay the fees for the new account creation
            .build_and_seal(&source_account_sk),
        vec![source_account_proof],
    );

    // Check that the previous transaction failed because of an address collision.
    assert_reject_reason(overwriting_tx, RuntimeError::ComponentAlreadyExists {
        address: source_account,
    });

    let store = test.read_only_state_store();
    let account = store.get_account(source_account).unwrap();
    let vault = account.get_vault_by_resource(&XTR).unwrap();
    // Double check that the source account was not overwritten due to the address collision, if it was, then we'd have
    // no vaults
    let vault = store.get_vault(&vault.vault_id()).expect("no vaults");
    assert_eq!(vault.balance(), TemplateTest::FUNDED_ACCOUNT_INITIAL_BALANCE);
}

#[test]
fn create_account_is_idempotent() {
    let mut test = TemplateTest::new_builtin_only();

    // Create initial account with faucet funds
    let (source_account, source_account_proof, source_account_sk) = test.create_funded_account();
    let source_account_pk = RistrettoPublicKey::from_secret_key(&source_account_sk);

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            // Create component with the same ID
            .create_account(source_account_pk.to_byte_type())
            // Signed by source account so that it can pay the fees for the new account creation
            .build_and_seal(&source_account_sk),
        vec![source_account_proof],
    );

    assert!(
        result
            .finalize
            .events
            .iter()
            .all(|e| e.topic() != "std.component.created")
    );

    let store = test.read_only_state_store();
    let account = store.get_account(source_account).unwrap();
    let vault = account.get_vault_by_resource(&XTR).unwrap();
    // Double check that the source account was not overwritten due to the address collision, if it was, then we'd have
    // no vaults
    let vault = store.get_vault(&vault.vault_id()).expect("no vaults");
    assert_eq!(vault.balance(), TemplateTest::FUNDED_ACCOUNT_INITIAL_BALANCE);
}

#[test]
fn create_account_is_idempotent_with_deposit() {
    let mut test = TemplateTest::new_builtin_only();

    // Create initial account with faucet funds
    let (source_account, source_account_proof, source_account_sk) = test.create_empty_account();
    let source_account_pk = RistrettoPublicKey::from_secret_key(&source_account_sk);

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(xtr_faucet_component(), "take", args![1000])
            .put_last_instruction_output_on_workspace("bucket")
            // Create component with the same ID
            .create_account_with_bucket(source_account_pk.to_byte_type(), "bucket")
            // Signed by source account so that it can pay the fees for the new account creation
            .build_and_seal(&source_account_sk),
        vec![source_account_proof],
    );

    assert!(
        result
            .finalize
            .events
            .iter()
            .all(|e| e.topic() != "std.component.created")
    );

    let store = test.read_only_state_store();
    let account = store.get_account(source_account).unwrap();
    let vault = account.get_vault_by_resource(&XTR).unwrap();
    let vault = store.get_vault(&vault.vault_id()).unwrap();
    assert_eq!(vault.balance(), 1000u64);
}

#[test]
fn gasless() {
    let mut test = TemplateTest::new_builtin_only();
    test.enable_fees();

    // Create initial account with faucet funds
    let (fee_account, fee_account_proof, fee_account_sk) = test.create_funded_account();
    let (user_account, user_account_proof, user_account_sk) = test.create_funded_account();
    let (user2_account, _, _) = test.create_empty_account();

    let fee_account_pk = RistrettoPublicKey::from_secret_key(&fee_account_sk);

    test.execute_expect_success(
        Transaction::builder_localnet()
            .pay_fee_from_component(fee_account, 1000u64)
            .call_method(user_account, "withdraw", args![XTR, 100])
            .put_last_instruction_output_on_workspace("b")
            .call_method(user2_account, "deposit", args![Workspace("b")])
            .finish()
            .add_signer(&fee_account_pk.to_byte_type(), &user_account_sk)
            .seal(&fee_account_sk),
        vec![fee_account_proof, user_account_proof],
    );

    let vaults = test
        .read_only_state_store()
        .get_vaults_for_account(user2_account)
        .unwrap();
    assert_eq!(vaults.get(&XTR).unwrap().balance(), 100u64);
}

#[test]
fn custom_access_rules() {
    let mut test = TemplateTest::new_builtin_only();

    // First we create a account with a custom rule that anyone can withdraw
    let (owner_proof, public_key, secret_key) = test.create_owner_proof();

    let access_rules = ComponentAccessRules::new()
        .add_method_rule("balance", rule!(allow_all))
        .add_method_rule("get_balances", rule!(allow_all))
        .add_method_rule("deposit", rule!(allow_all))
        .add_method_rule("deposit_all", rule!(allow_all))
        // We are going to make it so anyone can withdraw
        .default(rule!(allow_all));

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(xtr_faucet_component(), "take", args![1000])
            .put_last_instruction_output_on_workspace("bucket")
            // Create component with the same ID
            .create_account_custom(
                public_key.to_byte_type(),
                None,
                Some(access_rules),
                Some("bucket"),
            )
            // Signed by source account so that it can pay the fees for the new account creation
            .build_and_seal(&secret_key),
        vec![owner_proof],
    );
    let user_account = *test
        .read_only_state_store()
        .all_accounts()
        .unwrap()
        .keys()
        .next()
        .unwrap();

    // We create another account and we we will withdraw from the custom one
    let (user2_account, user2_account_proof, user2_secret_key) = test.create_funded_account();
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(user_account, "withdraw", args![XTR, 100])
            .put_last_instruction_output_on_workspace("b")
            .call_method(user2_account, "deposit", args![Workspace("b")])
            .build_and_seal(&user2_secret_key),
        vec![user2_account_proof],
    );
}

#[test]
fn take_from_bucket() {
    let mut test = TemplateTest::new(CRATE_PATH, Vec::<&str>::new());

    let faucet_template = test.get_template_address("TestFaucet");
    let (alice, _proof, _alice_sk) = test.create_empty_account();
    let (bob, _proof, _bob_sk) = test.create_empty_account();

    let initial_supply = Amount::from(1_000_000_000_000u64);
    test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("faucet")
            .call_function(faucet_template, "mint_with_opts", args![
                initial_supply,
                "faucet".to_string(),
                Workspace("faucet"),
            ])
            .call_method("faucet", "take_free_coins_custom", args![1000])
            .put_last_instruction_output_on_workspace("free_coins")
            .take_from_bucket("free_coins", 100u64, "foo_bucket")
            // Take all to test what happens when free_coins is empty (should not fail due to dangling buckets)
            .take_from_bucket("free_coins", 900u64, "bar_bucket")
            .call_method(alice, "deposit", args![Workspace("foo_bucket")])
            .call_method(bob, "deposit", args![Workspace("bar_bucket")])
            .build_and_seal(test.secret_key()),
        vec![test.owner_proof()],
    );

    let resources = test.read_only_state_store().get_all_resources().unwrap();
    let faucet_resource = resources
        .iter()
        .find(|(_, r)| r.token_symbol() == Some("faucet"))
        .map(|(addr, _)| *addr)
        .unwrap();

    let alice_acc = test.read_only_state_store().get_account(alice).unwrap();
    let bob_acc = test.read_only_state_store().get_account(bob).unwrap();
    let alice_balance = test
        .read_only_state_store()
        .get_vault(&alice_acc.get_vault_by_resource(&faucet_resource).unwrap().vault_id())
        .unwrap()
        .balance();
    let bob_balance = test
        .read_only_state_store()
        .get_vault(&bob_acc.get_vault_by_resource(&faucet_resource).unwrap().vault_id())
        .unwrap()
        .balance();

    assert_eq!(alice_balance, 100);
    assert_eq!(bob_balance, 900);
}
