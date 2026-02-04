//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use ootle_byte_type::ToByteType;
use tari_ootle_transaction::{Transaction, args};
use tari_template_lib::types::{Amount, ComponentAddress, NonFungibleId, ResourceAddress, VaultId};
use tari_template_test_tooling::{
    TemplateTest,
    support::confidential::{generate_confidential_output_statement, generate_withdraw_proof},
};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn it_recalls_all_resource_types() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/recall"]);
    let recall_template = test.get_template_address("Recall");
    let (account, _, _) = test.create_empty_account();

    let (mut initial_supply, mask, _) = generate_confidential_output_statement(1000, None);
    initial_supply.output_revealed_amount = Amount::from(1000u64);

    let result = test.execute_expect_success(
        Transaction::builder_localnet()
            .call_function(recall_template, "new", args![initial_supply])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let recall_component: ComponentAddress = result.finalize.execution_results[0].get_value("$.0").unwrap().unwrap();
    let fungible_resource: ResourceAddress = result.finalize.execution_results[0].get_value("$.1").unwrap().unwrap();
    let non_fungible_resource: ResourceAddress =
        result.finalize.execution_results[0].get_value("$.2").unwrap().unwrap();
    let confidential_resource: ResourceAddress =
        result.finalize.execution_results[0].get_value("$.3").unwrap().unwrap();
    let stealth_resource: ResourceAddress = result.finalize.execution_results[0].get_value("$.4").unwrap().unwrap();

    let withdraw = generate_withdraw_proof(&mask, 10, Some(980), 10u64);
    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(recall_component, "withdraw_some", args![withdraw.proof])
            .put_last_instruction_output_on_workspace("buckets")
            .call_method(account, "deposit", args![Workspace("buckets.0")])
            .call_method(account, "deposit", args![Workspace("buckets.1")])
            .call_method(account, "deposit", args![Workspace("buckets.2")])
            .call_method(account, "deposit", args![Workspace("buckets.3")])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let vaults: BTreeMap<ResourceAddress, VaultId> = test.extract_component_value(account, "$.vaults");
    let fungible_vault = vaults[&fungible_resource];
    let non_fungible_vault = vaults[&non_fungible_resource];
    let confidential_vault = vaults[&confidential_resource];
    let stealth_vault = vaults[&stealth_resource];

    let commitment = withdraw
        .to_commitment_for_output(Amount::from(10u64))
        .unwrap()
        .to_byte_type();

    test.execute_expect_success(
        Transaction::builder_localnet()
            .call_method(recall_component, "recall_fungible", args![fungible_vault, 6])
            .call_method(recall_component, "recall_non_fungibles", args![non_fungible_vault, [
                NonFungibleId::from_u32(1)
            ]])
            .call_method(recall_component, "recall_confidential", args![
                confidential_vault,
                [commitment],
                4
            ])
            .call_method(recall_component, "recall_stealth", args![stealth_vault, 8])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let vault = test.read_only_state_store().get_vault(&fungible_vault).unwrap();
    let fungible_balance = vault.balance();
    assert_eq!(fungible_balance, Amount::from(4u64));

    let vault = test.read_only_state_store().get_vault(&non_fungible_vault).unwrap();
    let non_fungible_balance = vault.balance();
    assert_eq!(non_fungible_balance, Amount::from(1u64));

    let vault = test.read_only_state_store().get_vault(&confidential_vault).unwrap();
    let confidential_balance = vault.balance();
    assert_eq!(confidential_balance, Amount::from(6u64));

    let vault = test.read_only_state_store().get_vault(&stealth_vault).unwrap();
    let stealth_balance = vault.balance();
    assert_eq!(stealth_balance, Amount::from(2u64));
}
