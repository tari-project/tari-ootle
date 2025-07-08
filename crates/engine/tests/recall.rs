//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use tari_engine_types::ToByteType;
use tari_template_lib::{
    models::{NonFungibleId, ResourceAddress, VaultId},
    prelude::ComponentAddress,
    types::Amount,
};
use tari_template_test_tooling::{
    support::confidential::{generate_confidential_output_statement, generate_withdraw_proof},
    TemplateTest,
};
use tari_transaction::{args, Transaction};

#[test]
fn it_recalls_all_resource_types() {
    let mut test = TemplateTest::new(["tests/templates/recall"]);
    let recall_template = test.get_template_address("Recall");
    let (account, _, _) = test.create_empty_account();

    let (mut initial_supply, mask, _) = generate_confidential_output_statement(Amount::from(1000), None);
    initial_supply.output_revealed_amount = Amount::from(1000);

    let result = test.execute_expect_success(
        Transaction::builder()
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

    let withdraw = generate_withdraw_proof(&mask, 10, Some(980), 10);
    test.execute_expect_success(
        Transaction::builder()
            .call_method(recall_component, "withdraw_some", args![withdraw.proof])
            .put_last_instruction_output_on_workspace("buckets")
            .call_method(account, "deposit", args![Workspace("buckets.0")])
            .call_method(account, "deposit", args![Workspace("buckets.1")])
            .call_method(account, "deposit", args![Workspace("buckets.2")])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let vaults: BTreeMap<ResourceAddress, VaultId> = test.extract_component_value(account, "$.vaults");
    let fungible_vault = vaults[&fungible_resource];
    let non_fungible_vault = vaults[&non_fungible_resource];
    let confidential_vault = vaults[&confidential_resource];

    let commitment = withdraw
        .to_commitment_for_output(Amount::from(10))
        .unwrap()
        .to_byte_type();

    let result = test.execute_expect_success(
        Transaction::builder()
            .call_method(recall_component, "recall_fungible", args![fungible_vault, Amount(6)])
            .call_method(recall_component, "recall_non_fungibles", args![non_fungible_vault, [
                NonFungibleId::from_u32(1)
            ]])
            .call_method(recall_component, "recall_confidential", args![
                confidential_vault,
                [commitment],
                Amount(4)
            ])
            .call_method(recall_component, "get_balances", args![])
            .call_method(account, "balance", args![fungible_resource])
            .call_method(account, "balance", args![non_fungible_resource])
            .call_method(account, "balance", args![confidential_resource])
            .build_and_seal(test.secret_key()),
        vec![],
    );

    let fungible_balance = result.finalize.execution_results[4].decode::<Amount>().unwrap();
    assert_eq!(fungible_balance, Amount::from(4));

    let non_fungible_balance = result.finalize.execution_results[5].decode::<Amount>().unwrap();
    assert_eq!(non_fungible_balance, Amount::from(1));

    let confidential_balance = result.finalize.execution_results[6].decode::<Amount>().unwrap();
    assert_eq!(confidential_balance, Amount::from(6));
}
