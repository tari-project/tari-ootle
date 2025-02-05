# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

# TODO: This feature is currently ignored because the fee claiming needs to be reworked

@claim_fees
Feature: Claim Fees

  @serial @fixed
  Scenario: Claim validator fees
    # Initialize a base node, wallet, miner and VN
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    # Initialize an indexer
    Given an indexer IDX connected to base node BASE

    # Initialize the wallet daemon
    Given a wallet daemon WALLET_D connected to indexer IDX
    When I create a key named K1 for WALLET_D

    # Initialize a VN
    Given a seed validator node VN connected to base node BASE and wallet daemon WALLET_D using claim fee key K1
    When miner MINER mines 4 new blocks
    When wallet WALLET has at least 5000 T
    When validator node VN sends a registration transaction to base wallet WALLET
    When miner MINER mines 16 new blocks
    Then VN has scanned to height 17
    And indexer IDX has scanned to height 17
    Then the validator node VN is listed as registered

    When indexer IDX connects to all other validators

    # Run some transactions to generate fees
    When I create an account ACC1 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACC2 via the wallet daemon WALLET_D with 10000 free coins using key K1
    When I create an account ACC3 via the wallet daemon WALLET_D with 10000 free coins

    # Progress to the next epoch
    When miner MINER mines 10 new blocks
    Then VN has scanned to height 27

    When I check the balance of ACC2 on wallet daemon WALLET_D the amount is at most 9700

    # Claim fees into ACC2
    When I claim fees for validator VN into account ACC2 using the wallet daemon WALLET_D

    # Check that there is a net gain
    # There is a small fee claim (observed: 341 at the time of comment). It is difficult to figure out the exact balance after transaction fees.
    When I check the balance of ACC2 on wallet daemon WALLET_D the amount is at least 9800

  @serial @fixed
  Scenario: Prevent double claim of validator fees
    # Initialize a base node, wallet, miner and VN
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    # Initialize an indexer
    Given an indexer IDX connected to base node BASE

    # Initialize the wallet daemon
    Given a wallet daemon WALLET_D connected to indexer IDX
    When I create a key named K1 for WALLET_D

    # Initialize a VN
    Given a seed validator node VN connected to base node BASE and wallet daemon WALLET_D using claim fee key K1
    When miner MINER mines 4 new blocks
    When wallet WALLET has at least 10000 T
    When validator node VN sends a registration transaction to base wallet WALLET
    When miner MINER mines 16 new blocks
    Then VN has scanned to height 17
    And indexer IDX has scanned to height 17
    Then the validator node VN is listed as registered

    When indexer IDX connects to all other validators

    # Run some transactions to generate fees
    When I create an account ACC1 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACC2 via the wallet daemon WALLET_D with 10000 free coins using key K1
    When I create an account ACC3 via the wallet daemon WALLET_D with 10000 free coins
    When I create an account ACC4 via the wallet daemon WALLET_D with 10000 free coins

    # Progress to the next epoch
    When miner MINER mines 10 new blocks
    Then VN has scanned to height 27

    # Can't claim fees with different account
    When I claim fees for validator VN into account ACC1 using the wallet daemon WALLET_D, it fails

    # Claim fees into ACC2
    When I check the balance of ACC2 on wallet daemon WALLET_D the amount is at most 9700
    When I claim fees for validator VN into account ACC2 using the wallet daemon WALLET_D
    When I check the balance of ACC2 on wallet daemon WALLET_D the amount is at least 9800

    # Claim fees into ACC2
  # This does not fail because the previous fee claim added fees to the fee pool of the validator
#    When I claim fees for validator VN into account ACC2 using the wallet daemon WALLET_D, it fails
