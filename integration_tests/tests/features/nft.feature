# Copyright 2024 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@nft
Feature: NFTs

  Scenario: Mint, mutate and burn non fungible tokens
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Publish the "basic_nft" template
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins
    When wallet daemon WALLET_D publishes the template "basic_nft" using account ACC

    ###### Scenario
    # Create two accounts to deposit the minted NFTs
    When I create an account ACC1 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACC2 via the wallet daemon WALLET_D with 10000 free coins

    # Mint a basic NFT
    When I mint a new non fungible token NFT_X on ACC1 using wallet daemon WALLET_D

    # Check that a new NFT_X has been minted for ACC1
    # TODO: investigate flaky test
    #When I list all non fungible tokens on ACC1 using wallet daemon WALLET_D the amount is 1

    # Create instance of the basic NFT template
    When I call function "new" on template "basic_nft" using account ACC1 to pay fees via wallet daemon WALLET_D named "NFT"

    # Submit a transaction with NFT operations
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "NFT, ACC1, ACC2" named "TX1"
  ```
  let sparkle_nft = global!["NFT/components/SparkleNft"];
  let sparkle_res = global!["NFT/resources/0"];
  let mut acc1 = global!["ACC1/components/Account"];
  let mut acc2 = global!["ACC2/components/Account"];

  // mint a new nft with random id
  let nft_bucket = sparkle_nft.mint("NFT1", "http://example.com");
  acc1.deposit(nft_bucket);

  // mint a new nft with specific id
  let nft_bucket = sparkle_nft.mint_specific(NonFungibleId("SpecialNft"), "NFT2", "http://example.com");
  acc1.deposit(nft_bucket);

  // transfer nft between accounts
  let acc_bucket = acc1.withdraw_non_fungible(sparkle_res, NonFungibleId("SpecialNft"));
  acc2.deposit(acc_bucket);

  // mutate a nft
  sparkle_nft.inc_brightness(NonFungibleId("SpecialNft"), 10u32);

  // burn a nft
  let nft_bucket = sparkle_nft.mint_specific(NonFungibleId("Burn!"), "NFT3", "http://example.com");
  acc1.deposit(nft_bucket);
  let acc_bucket = acc1.withdraw_non_fungible(sparkle_res, NonFungibleId("Burn!"));
  sparkle_nft.burn(acc_bucket);
  ```


  Scenario: Create resource and mint in one transaction
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Publish the "basic_nft" template
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins
    When wallet daemon WALLET_D publishes the template "basic_nft" using account ACC

    ###### Scenario
    # Create an account to deposit the minted NFT
    When I create an account ACC1 via the wallet daemon WALLET_D with 10000 free coins

    # Create a new BasicNft component and mint in the same transaction.
    # Note the updated NFT address format or parsing the manifest will fail.
    When I call function "new_with_initial_nft" on template "basic_nft" using account ACC1 to pay fees via wallet daemon WALLET_D with args "nft_str_1000" named "NFT"

    # Check that the initial NFT was actually minted by trying to deposit it into an account
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "NFT, ACC1" named "TX1"
  ```
  let sparkle_nft = global!["NFT/components/SparkleNft"];
  let mut acc1 = global!["ACC1/components/Account"];

  // get the initailly NFT from the component's vault
  let nft_bucket = sparkle_nft.take_initial_nft();
  acc1.deposit(nft_bucket);
  ```
