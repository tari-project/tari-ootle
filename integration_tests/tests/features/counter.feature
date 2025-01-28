# Copyright 2024 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@counter
Feature: Counter template

  Scenario: Counter template registration and invocation once

    # Initialize a base node, wallet, miner and VN
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    # Initialize a validator node
    Given a validator node VN connected to base node BASE and wallet daemon WALLET_D

    # Fund wallet to send VN registration tx
    When miner MINER mines 10 new blocks
    When wallet WALLET has at least 2000 T
    When validator node VN sends a registration transaction to base wallet WALLET
    When miner MINER mines 26 new blocks
    Then the validator node VN is listed as registered

    # Initialize indexer and connect wallet daemon
    Given an indexer IDX connected to base node BASE
    Given a wallet daemon WALLET_D connected to indexer IDX

    # Create the sender account
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins

    # Publish the "counter" template
    When wallet daemon WALLET_D publishes the template "counter" using account ACC

    # The initial value of the counter must be 0
    When I call function "new" on template "counter" using account ACC to pay fees via wallet daemon WALLET_D named "COUNTER"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "0"

    # Increase the counter
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"

    # Check that the counter has been increased
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "1"


  Scenario: Counter template registration and invocation multiple times

    # Initialize a base node, wallet, miner and VN
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    # Initialize a validator node
    Given a validator node VN connected to base node BASE and wallet daemon WALLET_D

    # Fund wallet to send VN registration tx
    When miner MINER mines 10 new blocks
    When wallet WALLET has at least 2000 T
    When validator node VN sends a registration transaction to base wallet WALLET
    When miner MINER mines 26 new blocks
    Then the validator node VN is listed as registered

    # Initialize indexer and connect wallet daemon
    Given an indexer IDX connected to base node BASE
    Given a wallet daemon WALLET_D connected to indexer IDX

    # Create the sender account
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins

    # Publish the "counter" template
    When wallet daemon WALLET_D publishes the template "counter" using account ACC

    # The initial value of the counter must be 0
    When I call function "new" on template "counter" using account ACC to pay fees via wallet daemon WALLET_D named "COUNTER"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "0"

    # Increase and check the counter
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "1"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "2"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "3"
