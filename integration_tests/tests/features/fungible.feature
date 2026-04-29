# Copyright 2024 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@fungible
Feature: Fungible tokens

  Scenario: Mint fungible tokens
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Publish faucet template
    When I create an account ACC via the wallet daemon WALLET_D with 2 XTR
    When wallet daemon WALLET_D publishes the template "faucet" using account ACC

    ##### Scenario
    # Create two accounts to test deposit the tokens
    When I create an account ACC1 via the wallet daemon WALLET_D with 1 XTR
    When I create an account ACC2 via the wallet daemon WALLET_D with 1 XTR

    # Create a new faucet component
    When I call function "mint" on template "faucet" using account ACC1 to pay fees via wallet daemon WALLET_D with args "amount_10000" named "FAUCET"

    # Deposit tokens in first account
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, ACC1" named "TX1"
  """
  let faucet = global!["FAUCET/components/faucet"];
  let mut acc1 = global!["ACC1/accounts/ACC1"];

  // get tokens from the faucet
  let faucet_bucket = faucet.take_free_coins();
  acc1.deposit(faucet_bucket);
  """

    # Move tokens from first to second account
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, TX1, ACC2" named "TX2"
  """
  let mut acc1 = global!["TX1/accounts/ACC1"];
  let mut acc2 = global!["ACC2/accounts/ACC2"];
  let faucet_resource = global!["FAUCET/resources/FAUCET"];

  // Withdraw 50 of the tokens and send them to acc2
  let tokens = acc1.withdraw(faucet_resource, Amount(50));
  acc2.deposit(tokens);
  acc2.balance(faucet_resource);
  acc1.balance(faucet_resource);
  """
