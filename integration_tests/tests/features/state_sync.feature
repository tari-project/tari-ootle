# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@state_sync
Feature: State Sync

  Scenario: New validator node registers and syncs
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Submit a few transactions
    When I create an account ACC1 via the wallet daemon WALLET_D with 10000 XTR
    When I create an account UNUSED1 via the wallet daemon WALLET_D
    When I create an account UNUSED2 via the wallet daemon WALLET_D
    When I create an account UNUSED3 via the wallet daemon WALLET_D

    # When I wait for validator VN has leaf block height of at least 15

    # Start a new VN that needs to sync
    Given a validator node VN2 connected to base node BASE_NODE
    Given validator VN2 nodes connect to all other validators
    When indexer INDEXER connects to all other validators

    When validator node VN2 sends a registration transaction to base wallet MINOTARI_WALLET
    Then miner MINER mines to the next epoch
    Then the validator node VN2 is listed as registered
    Then the validator node VN has started epoch 4
    Then VN2 has scanned to at least height 40
    Then the validator node VN2 has started epoch 4

    When I wait for validator VN has leaf block height of at least 1 at epoch 4
    When I wait for validator VN2 has leaf block height of at least 1 at epoch 4

    When I create an account ACC4 via the wallet daemon WALLET_D with 2 XTR
    When I create an account ACC2 via the wallet daemon WALLET_D with 2 XTR

    When I wait for validator VN has leaf block height of at least 5 at epoch 4
    When I wait for validator VN2 has leaf block height of at least 5 at epoch 4
