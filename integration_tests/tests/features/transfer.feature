# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@transfer
Feature: Account transfers

  Scenario: Transfer tokens to account that does not previously exist
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Publish the "fauset" template
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins
    When wallet daemon WALLET_D publishes the template "faucet" using account ACC

    # Create the sender account
    When I create an account ACCOUNT via the wallet daemon WALLET_D with 10000 free coins

    # Create a new Faucet component
    When I call function "mint" on template "faucet" using account ACCOUNT to pay fees via wallet daemon WALLET_D with args "amount_10000" named "FAUCET"

    # Burn some tari in the base layer to have funds for fees in the sender account
    When I burn 10T on wallet NETWORK_CONSOLE_WALLET to proof BURN_PROOF for wallet daemon WALLET_D
    When miner NETWORK_MINER mines 13 new blocks
    Then VN has scanned to at least height 40
    Then indexer NETWORK_INDEXER has scanned to at least height 40

    When I convert commitment in proof BURN_PROOF into COMM_ADDRESS address
    Then validator node VN has state at COMM_ADDRESS within 20 seconds
    When I claim burn BURN_PROOF and spend it into account ACCOUNT using wallet daemon WALLET_D

    # Wait for the wallet daemon account monitor to update the sender account information

    # Fund the sender account with faucet tokens
    When I print the cucumber world
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, ACCOUNT" named "TX1"
  ```
  let faucet = global!["FAUCET/components/TestFaucet"];
  let mut acc1 = global!["ACCOUNT/components/Account"];

  // get tokens from the faucet
  let faucet_bucket = faucet.take_free_coins();
  acc1.deposit(faucet_bucket);
  ```

    # Wait for the wallet daemon account monitor to update the sender account information

    # TODO: remove the wait
    When I wait 5 seconds
    When I check the balance of ACCOUNT on wallet daemon WALLET_D the amount is at least 10000
    # Do the transfer from ACCOUNT to the second account (which does not exist yet in the network)
    When I create a new key pair KEY_ACC_2
    When I print the cucumber world
    When I transfer 50 tokens of resource FAUCET/resources/0 from account ACCOUNT to public key KEY_ACC_2 via the wallet daemon WALLET_D named TRANSFER

    When I print the cucumber world
    # Check that ACC_2 component was created and has funds
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, TRANSFER" named "TX2"
  ```
  let mut acc2 = global!["TRANSFER/components/Account"];
  let faucet_resource = global!["FAUCET/resources/0"];
  acc2.balance(faucet_resource);
  ```
    When I print the cucumber world

  Scenario: Transfer tokens to existing account
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Publish the "fauset" template
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins
    When wallet daemon WALLET_D publishes the template "faucet" using account ACC

    # Create the sender account with some tokens
    When I create an account ACCOUNT_1 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACCOUNT_2 via the wallet daemon WALLET_D

    # Create a new Faucet component
    When I call function "mint" on template "faucet" using account ACCOUNT_1 to pay fees via wallet daemon WALLET_D with args "amount_10000" named "FAUCET"

    # Burn some tari in the base layer to have funds for fees in the sender account
    When I burn 10T on wallet NETWORK_CONSOLE_WALLET to proof BURN_PROOF for wallet daemon WALLET_D
    When miner NETWORK_MINER mines 13 new blocks
    Then VN has scanned to at least height 40
    Then indexer IDX has scanned to at least height 40

    When I convert commitment in proof BURN_PROOF into COMM_ADDRESS address
    Then validator node VN has state at COMM_ADDRESS within 20 seconds
    When I claim burn BURN_PROOF and spend it into account ACCOUNT_1 using wallet daemon WALLET_D

    # Wait for the wallet daemon account monitor to update the sender account information

    # Fund the sender account with faucet tokens
    When I print the cucumber world
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, ACCOUNT_1" named "TX1"
  ```
  let faucet = global!["FAUCET/components/TestFaucet"];
  let mut acc1 = global!["ACCOUNT_1/components/Account"];

  // get tokens from the faucet
  let faucet_bucket = faucet.take_free_coins();
  acc1.deposit(faucet_bucket);
  ```

    When I wait 3 seconds

    # Do the transfer from ACCOUNT_1 to another existing account
    When I transfer 50 tokens of resource FAUCET/resources/0 from account ACCOUNT_1 to public key ACCOUNT_2 via the wallet daemon WALLET_D named TRANSFER

    # Check that ACCOUNT_2 component now has funds
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, ACCOUNT_2" named "TX2"
  ```
  let mut acc2 = global!["ACCOUNT_2/components/Account"];
  let faucet_resource = global!["FAUCET/resources/0"];
  acc2.balance(faucet_resource);
  ```
    When I print the cucumber world

  Scenario: Confidential transfer to account that does not previously exist
    Given a network with registered validator VN and wallet daemon WALLET_D

    Then VN has scanned to at least height 27
    Then indexer NETWORK_INDEXER has scanned to at least height 27

    # Create the sender account
    When I create an account ACC_1 via the wallet daemon WALLET_D with 10000 free coins

    When I check the balance of ACC_1 on wallet daemon WALLET_D the amount is at least 9000
    # Do the transfer from ACC_1 to the second account (which does not exist yet in the network)
    When I create a new key pair KEY_ACC_2
    When I do a confidential transfer of 50 from account ACC_1 to public key KEY_ACC_2 via the wallet daemon WALLET_D named TRANSFER

    When I print the cucumber world
