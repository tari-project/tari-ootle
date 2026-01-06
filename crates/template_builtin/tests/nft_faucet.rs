//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::commit_result::ExecuteResult;
use tari_template_lib::{
    constants::{NFT_FAUCET_COMPONENT_ADDRESS, NFT_FAUCET_RESOURCE_ADDRESS},
    models::{ComponentAddress, NonFungibleAddress},
    prelude::Metadata,
    resource::TOKEN_SYMBOL,
};
use tari_template_test_tooling::TemplateTest;
use tari_transaction::{args, Transaction};

#[test]
fn basic_nft_mint() {
    // setup the test
    let mut test = TemplateTest::new_no_templates();

    // create a user account
    let (owner_component_address, owner_token, _) = test.create_funded_account();

    // mint a new AccountNft
    let mut metadata = Metadata::new();
    metadata.insert(TOKEN_SYMBOL, "ACCNFT");
    metadata.insert("name", "my_custom_nft");
    metadata.insert("brightness", "100");

    mint_faucet_nft(&mut test, owner_component_address, owner_token.clone(), metadata).expect_success();

    let vault = test
        .read_only_state_store()
        .get_vaults_for_account(owner_component_address)
        .unwrap()
        .get(&NFT_FAUCET_RESOURCE_ADDRESS)
        .cloned()
        .unwrap();
    let bucket_nfts = vault.get_non_fungible_ids();
    assert_eq!(bucket_nfts.len(), 1);
}

#[test]
fn mint_multiple_times() {
    // setup the test
    let mut account_nft_template_test = TemplateTest::new_no_templates();

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
        Transaction::builder_localnet()
            .call_method(NFT_FAUCET_COMPONENT_ADDRESS, "mint", args![1, metadata])
            .put_last_instruction_output_on_workspace("my_nft")
            .call_method(account, "deposit", args![Workspace("my_nft")]),
        vec![owner_token],
    )
    .unwrap_success()
}
