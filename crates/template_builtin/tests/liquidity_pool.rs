//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{AccessRule, OwnerRule, constants::XTR, metadata};
use tari_template_test_tooling::TemplateTest;

const TEMPLATE_NAME: &str = "TwoResourceLiquidityPool";

#[test]
fn initial_contribution_and_redeem() {
    // Setup
    // NOTE: builtin templates are automatically added to the test environment. No need to specify template paths.
    let mut test = TemplateTest::new_no_templates();
    let template_address = test.get_template_address(TEMPLATE_NAME);

    let (faucet_component, faucet_resource) = test.create_test_faucet_component(1_000_000_000_000u64);

    let (user1, _user1_proof, user1_secret) = test.create_funded_account();

    // ACT 1: Create liquidity pool and contribute liquidity
    test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("pool")
            .call_function(template_address, "create", args![
                OwnerRule::OwnedBySigner,
                AccessRule::AllowAll,
                XTR,
                faucet_resource,
                metadata!["name" => "XTR-Stablecoin Liquidity Pool"],
                Workspace("pool"),
            ])
            .call_method(faucet_component, "take_free_coins_custom", args![2000])
            .put_last_instruction_output_on_workspace("faucet_coins")
            .call_method(user1, "withdraw", args![XTR, 1000])
            .put_last_instruction_output_on_workspace("xtr_coins")
            .call_method("pool", "contribute", args![
                Workspace("xtr_coins"),
                Workspace("faucet_coins")
            ])
            .put_last_instruction_output_on_workspace("pool_units")
            .call_method(user1, "deposit", args![Workspace("pool_units")])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &user1_secret)
            .seal(test.secret_key()),
        vec![],
    );

    // Assert ACT 1
    let store = test.read_only_state_store();
    let (pool_addr, pool_body) = store
        .get_components_by_template_address(template_address)
        .unwrap()
        .pop()
        .unwrap();
    let indexed = pool_body.body.to_indexed_well_known_types().unwrap();
    let vaults = indexed
        .vault_ids()
        .iter()
        .map(|id| store.get_vault(id).unwrap())
        .map(|v| (*v.resource_address(), v))
        .collect::<HashMap<_, _>>();
    let pool_unit_resx = indexed.resource_addresses().first().copied().unwrap();

    assert_eq!(vaults.get(&XTR).unwrap().balance(), 1000);
    assert_eq!(vaults.get(&faucet_resource).unwrap().balance(), 2000);

    let user_account = store.get_account(user1).unwrap();
    let vault = user_account.get_vault_by_resource(&pool_unit_resx).unwrap();
    let pool_units = store.get_vault(&vault.vault_id()).unwrap();
    assert!(pool_units.balance() >= 1000);

    // ACT 2: Redeem liquidity
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(user1, "withdraw", args![pool_unit_resx, pool_units.balance()])
            .put_last_instruction_output_on_workspace("redeem_pool_units")
            .call_method(pool_addr, "redeem", args![Workspace("redeem_pool_units")])
            .put_last_instruction_output_on_workspace("redeemed_coins")
            .assert_bucket_contains("redeemed_coins.0", XTR, 1000u64)
            .assert_bucket_contains("redeemed_coins.1", faucet_resource, 2000u64)
            .call_method(user1, "deposit", args![Workspace("redeemed_coins.0")])
            .call_method(user1, "deposit", args![Workspace("redeemed_coins.1")])
            .finish()
            .seal(&user1_secret),
        vec![],
    );

    // Assert ACT 2
    let store = test.read_only_state_store();
    let user_account = store.get_account(user1).unwrap();
    let xtr_vault = user_account.get_vault_by_resource(&XTR).unwrap();
    let xtr_coins = store.get_vault(&xtr_vault.vault_id()).unwrap();
    assert_eq!(xtr_coins.balance(), TemplateTest::FUNDED_ACCOUNT_INITIAL_BALANCE);
    let stablecoin_vault = user_account.get_vault_by_resource(&faucet_resource).unwrap();
    let stablecoin_coins = store.get_vault(&stablecoin_vault.vault_id()).unwrap();
    assert!(stablecoin_coins.balance() >= 2000);
}

#[test]
fn basic_constant_product_swap() {
    // Setup
    let mut test = TemplateTest::new_no_templates();
    let template_address = test.get_template_address(TEMPLATE_NAME);

    let (faucet_component, faucet_resource) = test.create_test_faucet_component(1_000_000_000_000u64);

    let (user1, _user1_proof, user1_secret) = test.create_funded_account();
    let (user2, _user2_proof, user2_secret) = test.create_empty_account();

    const INITIAL_XTR_AMOUNT: u64 = 1_000_000;
    const INITIAL_STABLECOIN_AMOUNT: u64 = 2_000_000;

    // ACT 1: Create liquidity pool and contribute liquidity
    test.execute_expect_success(
        Transaction::builder_localnet()
            .allocate_component_address("pool")
            .call_function(template_address, "create", args![
                OwnerRule::OwnedBySigner,
                AccessRule::AllowAll,
                XTR,
                faucet_resource,
                metadata!["name" => "XTR-Stablecoin Liquidity Pool"],
                Workspace("pool"),
            ])
            .call_method(faucet_component, "take_free_coins_custom", args![
                INITIAL_STABLECOIN_AMOUNT
            ])
            .put_last_instruction_output_on_workspace("faucet_coins")
            .call_method(user1, "withdraw", args![XTR, INITIAL_XTR_AMOUNT])
            .put_last_instruction_output_on_workspace("xtr_coins")
            .call_method("pool", "contribute", args![
                Workspace("xtr_coins"),
                Workspace("faucet_coins")
            ])
            .put_last_instruction_output_on_workspace("pool_units")
            .call_method(user1, "deposit", args![Workspace("pool_units")])
            // Give user some stablecoin for later
            .call_method(faucet_component, "take_free_coins_custom", args![
                1_000_000
            ])
            .put_last_instruction_output_on_workspace("user2_stablecoin")
            .call_method(user2, "deposit", args![Workspace("user2_stablecoin")])
            .finish()
            .add_signer(&test.to_public_key_bytes(), &user1_secret)
            .seal(test.secret_key()),
        vec![],
    );

    let store = test.read_only_state_store();
    let (pool_addr, _) = store
        .get_components_by_template_address(template_address)
        .unwrap()
        .pop()
        .unwrap();

    // ACT 2: Swap to pay fees
    test.enable_fees();
    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .with_fee_instructions_builder(|builder| {
                // User2 would like to swap stablecoin for XTR to pay fees
                builder
                    .call_method(user2, "withdraw", args![faucet_resource, 5000])
                    .put_last_instruction_output_on_workspace("coins_to_swap")
                    .call_method(pool_addr, "swap_constant_product", args![Workspace("coins_to_swap")])
                    .put_last_instruction_output_on_workspace("swapped_coins")
                    .call_method(user2, "deposit", args![Workspace("swapped_coins")])
                    .pay_fee_from_component(user2, 2000u64)
            })
            // Some instruction to make fees
            .call_method(user2, "create_proof_by_amount", args![faucet_resource, 1])
            .put_last_instruction_output_on_workspace("_proof")
            .drop_all_proofs_in_workspace()
            .finish()
            .seal(&user2_secret),
        vec![],
    );

    // Assert ACT 2
    let store = test.read_only_state_store();
    let user_account = store.get_account(user2).unwrap();
    let xtr_vault = user_account.get_vault_by_resource(&XTR).unwrap();
    let xtr_coins = store.get_vault(&xtr_vault.vault_id()).unwrap();
    let fees = result.finalize.fee_receipt.total_fees_paid();
    assert!(fees > 0);

    /// We duplicate the formula here to verify the template calculation.
    /// NOTE: this will not work for all values due to rounding errors.
    fn constant_product_calc(x_balance: u64, y_balance: u64, delta_x: u64) -> u64 {
        let x_balance = u128::from(x_balance);
        let y_balance = u128::from(y_balance);
        let delta_x = u128::from(delta_x);
        let delta_y = (y_balance * delta_x) / (x_balance + delta_x);
        u64::try_from(delta_y).unwrap()
    }

    assert_eq!(
        xtr_coins.balance(),
        constant_product_calc(INITIAL_STABLECOIN_AMOUNT, INITIAL_XTR_AMOUNT, 5000) - fees
    );
}
