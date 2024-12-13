# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@eviction
Feature: Eviction scenarios

  Scenario: Offline validator gets evicted
    # Initialize a base node, wallet, miner and several VNs
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    # Initialize an indexer
    Given an indexer IDX connected to base node BASE
    # Initialize the wallet daemon
    Given a wallet daemon WALLET_D connected to indexer IDX

    # Initialize VNs
    Given a seed validator node VN1 connected to base node BASE and wallet daemon WALLET_D
    Given a seed validator node VN2 connected to base node BASE and wallet daemon WALLET_D
    Given a seed validator node VN3 connected to base node BASE and wallet daemon WALLET_D
    Given a seed validator node VN4 connected to base node BASE and wallet daemon WALLET_D
    Given a seed validator node VN5 connected to base node BASE and wallet daemon WALLET_D

    When miner MINER mines 9 new blocks
    When wallet WALLET has at least 25000 T
    When validator node VN1 sends a registration transaction to base wallet WALLET
    When validator node VN2 sends a registration transaction to base wallet WALLET
    When validator node VN3 sends a registration transaction to base wallet WALLET
    When validator node VN4 sends a registration transaction to base wallet WALLET
    When validator node VN5 sends a registration transaction to base wallet WALLET

    When miner MINER mines 26 new blocks
    Then all validators have scanned to height 32
    And indexer IDX has scanned to height 32
    Then all validator nodes are listed as registered

    When indexer IDX connects to all other validators

    When all validator nodes have started epoch 3

    Then I stop validator node VN5

    # Submit some transactions to speed up block production
    Then I create an account ACC_1 via the wallet daemon WALLET_D with 10000 free coins
    Then I create an account ACC_2 via the wallet daemon WALLET_D with 10000 free coins
    Then I create an account ACC_3 via the wallet daemon WALLET_D with 10000 free coins
    Then I create an account ACC_4 via the wallet daemon WALLET_D with 10000 free coins
    Then I create an account ACC_5 via the wallet daemon WALLET_D with 10000 free coins

    Then I wait for VN1 to list VN5 as evicted in EVICT_PROOF
    Then I submit the eviction proof EVICT_PROOF to WALLET

    When miner MINER mines 10 new blocks
    Then all validators have scanned to height 42
    When all validator nodes have started epoch 4
    When miner MINER mines 10 new blocks
    Then all validators have scanned to height 52
    When all validator nodes have started epoch 5
    Then validator VN5 is not a member of the current network according to BASE
