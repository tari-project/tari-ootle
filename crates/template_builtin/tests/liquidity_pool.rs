//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_template_lib::{
    auth::{AccessRule, OwnerRule},
    constants::XTR,
    metadata,
};
use tari_template_test_tooling::{xtr_faucet_component, TemplateTest};
use tari_transaction::{args, Transaction};

#[test]
fn initial_contribution_and_redeem() {
    let mut test = TemplateTest::new_no_templates();

    // create a user account
    let (account_address, owner_token, owner_secret) = test.create_empty_account();

    // Create another resource
    let (faucet_component, faucet_resx) = test.create_test_faucet_component(100000);

    let template_addr = test.get_template_address("TwoResourceLiquidityPool");

    // Fund
    test.execute_expect_success(
        Transaction::builder_localnet()
            // Create the liquidity pool
            .allocate_component_address("pool")
            .call_function(template_addr, "create", args![
                OwnerRule::default(),
                AccessRule::AllowAll,
                XTR,
                faucet_resx,
                metadata!["name" => "TestPool".to_string()],
                Workspace("pool")
            ])
            // Fund the pool
            .call_method(faucet_component, "take_free_coins_custom", args![10000])
            .put_last_instruction_output_on_workspace("faucet_coins")
            .call_method(xtr_faucet_component(), "take", args![5000])
            .put_last_instruction_output_on_workspace("xtr_coins")
            .call_method(
                "pool",
                "contribute",
                args![Workspace("xtr_coins"), Workspace("faucet_coins")],
            )
            .put_last_instruction_output_on_workspace("pool_tokens")
            // Deposit pool tokens to user account
            .call_method(
                account_address,
                "deposit",
                args![Workspace("pool_tokens")],
            )
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let store = test.read_only_state_store();
    let vaults = store.get_vaults_for_account(account_address).unwrap();
    let vaults = vaults
        .into_iter()
        .map(|(addr, vault)| {
            let resource = store.get_resource(&addr).unwrap();
            (resource.token_symbol().unwrap().to_string(), vault)
        })
        .collect::<HashMap<_, _>>();

    // Note that the rounded down token amount is expected here - TODO: fix precision loss
    let expected_balance = (10000u64 * 5000).isqrt();
    let lp_vault = vaults.get("LP").unwrap();
    assert_eq!(lp_vault.balance(), expected_balance);

    // Redeem some liquidity
    let (pool_component, _) = store
        .get_components_by_template_address(template_addr)
        .unwrap()
        .pop()
        .unwrap();

    drop(store);

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(account_address, "withdraw", args![lp_vault.resource_address(), expected_balance])
            .put_last_instruction_output_on_workspace("pool_tokens")
            .call_method(
                pool_component,
                "redeem",
                args![Workspace("pool_tokens")],
            )
            .put_last_instruction_output_on_workspace("redeemed_coins")
            // Deposit redeemed coins to user account
            .call_method(account_address, "deposit", args![Workspace("redeemed_coins.0")])
            .call_method(account_address, "deposit", args![Workspace("redeemed_coins.1")])
            .build_and_seal(&owner_secret),
        vec![owner_token],
    );

    let store = test.read_only_state_store();
    let vaults = store.get_vaults_for_account(account_address).unwrap();
    let vaults = vaults
        .into_iter()
        .map(|(addr, vault)| {
            let resource = store.get_resource(&addr).unwrap();
            (resource.token_symbol().unwrap().to_string(), vault)
        })
        .collect::<HashMap<_, _>>();

    // Assert that we redeemed all XTR and faucet tokens back for the LP tokens
    let vault = vaults.get("tXTR").unwrap();
    assert_eq!(vault.balance(), 5000);
    let vault = vaults.get("faucets").unwrap();
    assert_eq!(vault.balance(), 10000);
    let vault = vaults.get("LP").unwrap();
    assert_eq!(vault.balance(), 0);
}
