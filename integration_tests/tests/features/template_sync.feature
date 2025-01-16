# Copyright 2025 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@serial
@template
Feature: Template syncing

  @dev
  Scenario: Template is synced for newly joined VNs
    # Initialize
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    Given a seed validator node SEED_VN connected to base node BASE and wallet daemon WALLET_D
    Given a validator node VN_1 connected to base node BASE and wallet daemon WALLET_D
    Given a validator node VN_2 connected to base node BASE and wallet daemon WALLET_D
    Given validator VN_1 nodes connect to all other validators

    # Register first VN
#    Given a validator node VN_1 connected to base node BASE and wallet daemon WALLET_D
    When miner MINER mines 10 new blocks
    When wallet WALLET has at least 2000 T
    When validator node VN_1 sends a registration transaction to base wallet WALLET
    When miner MINER mines 25 new blocks
    Then the validator node VN_1 is listed as registered

    # Indexer
    Given an indexer IDX connected to base node BASE
    Given a wallet daemon WALLET_D connected to indexer IDX

    # Create account and publish counter template
    When I create an account ACC via the wallet daemon WALLET_D with 2000000 free coins
    When wallet daemon WALLET_D publishes the template "counter" using account ACC
    Then the template "counter" is listed as registered by the validator node VN_1

    # Register second VN
#    Given a validator node VN_2 connected to base node BASE and wallet daemon WALLET_D
    When validator node VN_2 sends a registration transaction to base wallet WALLET
    When miner MINER mines 25 new blocks
    Then the validator node VN_2 is listed as registered
    Given validator VN_1 nodes connect to all other validators
    Given validator VN_2 nodes connect to all other validators

    # Check if we have the template published for VN 2
    Then the template "counter" is listed as registered by the validator node VN_2


