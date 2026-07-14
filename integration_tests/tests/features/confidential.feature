# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@confidential
Feature: Confidential transfers

  Scenario: Transfer confidential tokens between accounts
    Given a network with registered validator VN and wallet daemon WALLET_D

    When I create an account ACCOUNT_1 via the wallet daemon WALLET_D with 10000 XTR
    When I create an account ACCOUNT_2 via the wallet daemon WALLET_D with 10000 XTR

    # Publish a faucet for a confidential resource. Its supply is revealed, so the funds can be taken without
    # a confidential proof; the transfers below are what turn them into confidential outputs.
    When wallet daemon WALLET_D publishes the template "confidential_faucet" using account ACCOUNT_1
    When I call function "mint" on template "confidential_faucet" using account ACCOUNT_1 to pay fees via wallet daemon WALLET_D with args "amount_10000" named "FAUCET"

    # Fund ACCOUNT_1 with revealed funds of the confidential resource
    When I submit a transaction manifest via wallet daemon WALLET_D with inputs "FAUCET, ACCOUNT_1" named "TX1"
    """
    let faucet = global!["FAUCET/components/confidential_faucet"];
    let mut acc1 = global!["ACCOUNT_1/accounts/ACCOUNT_1"];
    let coins = faucet.take_free_coins(Amount(5000));
    acc1.deposit(coins);
    """
    When I check the balance of ACCOUNT_1 for resource FAUCET/resources/CONF on wallet daemon WALLET_D the amount is exactly 5000

    # Revealed -> confidential. The output must be encrypted to ACCOUNT_2's view key, otherwise ACCOUNT_2's
    # wallet cannot decrypt it and the funds do not show up in its confidential balance.
    When I do a confidential transfer of 1000 from ACCOUNT_1 to ACCOUNT_2 for resource FAUCET/resources/CONF selecting prefer-revealed inputs creating output TX2 via the wallet daemon WALLET_D
    When I check the confidential balance of ACCOUNT_2 on wallet daemon WALLET_D the amount is at least 1000

    # Confidential -> confidential. Spending that output downs its ConfidentialOutput substate, which the
    # wallet must declare as a transaction input.
    When I do a confidential transfer of 400 from ACCOUNT_2 to ACCOUNT_1 for resource FAUCET/resources/CONF selecting confidential inputs creating output TX3 via the wallet daemon WALLET_D
    When I check the confidential balance of ACCOUNT_1 on wallet daemon WALLET_D the amount is at least 400
