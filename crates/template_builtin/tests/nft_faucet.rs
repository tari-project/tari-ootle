//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::commit_result::ExecuteResult;
use tari_template_lib::{
    constants::NFT_FAUCET_COMPONENT_ADDRESS,
    models::{ComponentAddress, NonFungibleAddress, NonFungibleId},
    prelude::Metadata,
    resource::TOKEN_SYMBOL,
};
use tari_template_test_tooling::TemplateTest;
use tari_transaction::{args, Transaction};

#[test]
fn basic_nft_mint() {
    // setup the test
    let mut test = TemplateTest::new::<_, &str>([]);

    // create a user account
    let (owner_component_address, owner_token, _) = test.create_funded_account();

    // mint a new AccountNft
    let mut metadata = Metadata::new();
    metadata.insert(TOKEN_SYMBOL, "ACCNFT");
    metadata.insert("name", "my_custom_nft");
    metadata.insert("brightness", "100");

    let result = mint_faucet_nft(&mut test, owner_component_address, owner_token.clone(), metadata);
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

    // mint one nft
    let result = mint_faucet_nft(
        &mut account_nft_template_test,
        owner_component_address,
        owner_token.clone(),
        Metadata::new(),
    );
    assert!(result.finalize.result.is_accept());

    // mint a second nft
    let result = mint_faucet_nft(
        &mut account_nft_template_test,
        owner_component_address,
        owner_token.clone(),
        Metadata::new(),
    );
    assert!(result.finalize.result.is_accept());
}

fn mint_faucet_nft(
    test: &mut TemplateTest,
    account: ComponentAddress,
    owner_token: NonFungibleAddress,
    metadata: Metadata,
) -> ExecuteResult {
    test.build_and_execute(
        Transaction::builder()
            .call_method(NFT_FAUCET_COMPONENT_ADDRESS, "mint", args![Amount(1), metadata])
            .put_last_instruction_output_on_workspace("my_nft")
            .call_function(
                test.get_template_address("Account"),
                "get_non_fungible_ids_for_bucket",
                args![Workspace("my_nft")],
            )
            .call_method(account, "deposit", args![Workspace("my_nft")]),
        vec![owner_token],
    )
    .unwrap_success()
}
