//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{commit_result::ExecuteResult, instruction::Instruction};
use tari_template_lib::{
    call_args,
    models::{ComponentAddress, NonFungibleAddress, NonFungibleId},
    prelude::Metadata,
    resource::TOKEN_SYMBOL,
    types::TemplateAddress,
};
use tari_template_test_tooling::TemplateTest;
use tari_transaction::{args, Transaction};

#[test]
fn basic_nft_mint() {
    // setup the test
    let mut account_nft_template_test = TemplateTest::new::<_, &str>([]);

    // create a user account
    let (owner_component_address, owner_token, _) = account_nft_template_test.create_funded_account();

    // get the AccountNft template address
    let account_nft_template = account_nft_template_test.get_template_address("AccountNonFungible");

    // create the AccountNft component associated with the user account
    let result = create_nft_component(
        &mut account_nft_template_test,
        account_nft_template,
        owner_token.clone(),
    );
    assert!(result.finalize.result.is_accept());
    let nft_component_address: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();

    account_nft_template_test
        .build_and_execute(
            Transaction::builder().call_method(
                nft_component_address,
                "non_fungible_token_get_resource_address",
                args![],
            ),
            vec![],
        )
        .unwrap_success();

    // mint a new AccountNft
    let mut metadata = Metadata::new();
    metadata.insert(TOKEN_SYMBOL.to_string(), "ACCNFT".to_string());
    metadata.insert("name".to_string(), "my_custom_nft".to_string());
    metadata.insert("brightness".to_string(), "100".to_string());

    let result = mint_account_nft(
        &mut account_nft_template_test,
        nft_component_address,
        owner_component_address,
        owner_token.clone(),
        metadata,
    );
    assert!(result.finalize.result.is_accept());

    let bucket_nfts = result.finalize.execution_results[2]
        .decode::<Vec<NonFungibleId>>()
        .unwrap();
    assert_eq!(bucket_nfts.len(), 1);
}

#[test]
fn mint_multiple_times() {
    // setup the test
    let mut account_nft_template_test = TemplateTest::new::<_, &str>([]);

    // create a user account
    let (owner_component_address, owner_token, _) = account_nft_template_test.create_funded_account();

    // get the AccountNft template address
    let account_nft_template = account_nft_template_test.get_template_address("AccountNonFungible");

    // create the account nft component
    let result = create_nft_component(
        &mut account_nft_template_test,
        account_nft_template,
        owner_token.clone(),
    );
    assert!(result.finalize.result.is_accept());
    let nft_component_address: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();

    // mint one nft
    let result = mint_account_nft(
        &mut account_nft_template_test,
        nft_component_address,
        owner_component_address,
        owner_token.clone(),
        Metadata::new(),
    );
    assert!(result.finalize.result.is_accept());

    // mint a second nft
    let result = mint_account_nft(
        &mut account_nft_template_test,
        nft_component_address,
        owner_component_address,
        owner_token.clone(),
        Metadata::new(),
    );
    assert!(result.finalize.result.is_accept());
}

fn create_nft_component(
    test: &mut TemplateTest,
    nft_template: TemplateAddress,
    owner_token: NonFungibleAddress,
) -> ExecuteResult {
    test.execute_and_commit(
        vec![Instruction::CallFunction {
            address: nft_template,
            function: "create".to_string(),
            args: call_args![owner_token],
        }],
        vec![],
    )
    .unwrap()
}

fn mint_account_nft(
    test: &mut TemplateTest,
    nft_component: ComponentAddress,
    account: ComponentAddress,
    owner_token: NonFungibleAddress,
    metadata: Metadata,
) -> ExecuteResult {
    test.build_and_execute(
        Transaction::builder()
            .call_method(nft_component, "mint", args![metadata])
            .put_last_instruction_output_on_workspace("my_nft")
            .call_function(
                test.get_template_address("Account"),
                "get_non_fungible_ids_for_bucket",
                args![Variable("my_nft")],
            )
            .call_method(account, "deposit", args![Workspace("my_nft")]),
        vec![owner_token],
    )
    .unwrap_success()
}
