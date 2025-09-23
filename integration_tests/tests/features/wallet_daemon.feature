# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@wallet_daemon
Feature: Wallet Daemon

  Scenario: Create account and transfer faucets via wallet daemon
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Publish the "fauset" template
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins
    When wallet daemon WALLET_D publishes the template "faucet" using account ACC

        # Create two accounts to test sending the tokens
    When I create an account ACC_1 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACC_2 via the wallet daemon WALLET_D with 100000 free coins
    When I check the balance of ACC_2 on wallet daemon WALLET_D the amount is at least 10000

        # Create a new Faucet component
    When I call function "mint" on template "faucet" using account ACC_1 to pay fees via wallet daemon WALLET_D with args "10000" named "FAUCET"

        # Submit a transaction manifest
    When I print the cucumber world
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, ACC_1" named "TX1"
  ```
  let faucet = global!["FAUCET/components/TestFaucet"];
  let mut acc1 = global!["ACC_1/components/Account"];

  // get tokens from the faucet
  let faucet_bucket = faucet.take_free_coins();
  acc1.deposit(faucet_bucket);
  ```
    When I print the cucumber world

        # Submit a transaction manifest
    When I submit a transaction manifest via wallet daemon WALLET_D signed by the key of ACC_1 with inputs "FAUCET, TX1, ACC_2" named "TX2"
  ```
  let mut acc1 = global!["TX1/components/Account"];
  let mut acc2 = global!["ACC_2/components/Account"];
  let faucet_resource = global!["FAUCET/resources/0"];

  // Withdraw 50 of the tokens and send them to acc2
  let tokens = acc1.withdraw(faucet_resource, Amount(1000));
  acc2.deposit(tokens);
  acc2.balance(faucet_resource);
  acc1.balance(faucet_resource);
  ```
        # Check balances
        # `take_free_coins` deposits 10000 faucet tokens, allow up to 2000 in fees
    # TODO: this doesnt check the faucet tokens resource
#    When I check the balance of ACC_1 on wallet daemon WALLET_D the amount is exactly 0
#    When I wait for ACC_2 on wallet daemon WALLET_D to have balance eq 1000

  Scenario: Claim and transfer confidential assets via wallet daemon
    Given a network with registered validator VN and wallet daemon WALLET_D

        # When I create a component SECOND_LAYER_TARI of template "fees" on VN using "new"
    When I create an account ACCOUNT_1 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACCOUNT_2 via the wallet daemon WALLET_D

    When I burn 1000T on wallet NETWORK_CONSOLE_WALLET to proof BURN_PROOF for wallet daemon WALLET_D

        # unfortunately have to wait for this to get into the mempool....
    Then there is 1 transaction in the mempool of NETWORK_BASE_NODE within 10 seconds
    When miner NETWORK_MINER mines 13 new blocks
    Then VN has scanned to at least height 40

    When I wait for proof BURN_PROOF to confirm on wallet NETWORK_CONSOLE_WALLET

    When I claim burn BURN_PROOF and spend it into account ACCOUNT_1 using wallet daemon WALLET_D
    When I print the cucumber world
    When I check the confidential balance of ACCOUNT_1 on wallet daemon WALLET_D the amount is at least 10000

    Then I make a confidential transfer with amount 5 from ACCOUNT_1 to ACCOUNT_2 creating output OUTPUT_TX1 via the wallet_daemon WALLET_D

  Scenario: Create and mint account NFT
    # Initialize a base node, wallet, miner and VN
    Given a network with registered validator VAL_1 and wallet daemon WALLET_D

    # Create two accounts to test sending the tokens
    When I create an account ACC via the wallet daemon WALLET_D with 10000 free coins

    # Mint a new account NFT
    When I mint a new non fungible token NFT on ACC using wallet daemon WALLET_D
