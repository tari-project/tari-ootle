# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause
@claim_burn
Feature: Claim Burn

  @concurrent
#  @serial
  Scenario: Claim base layer burn funds with wallet daemon
    Given a network with registered validator VN and wallet daemon WALLET_D

    When I create an account ACC via the wallet daemon WALLET_D

    When I burn 10T on wallet MINOTARI_WALLET to proof BURN_PROOF for wallet daemon WALLET_D

    # unfortunately have to wait for this to get into the mempool....
    Then there is 1 transaction in the mempool of BASE_NODE within 10 seconds
    When miner MINER mines 5 new blocks

    When I wait for proof BURN_PROOF to confirm on wallet MINOTARI_WALLET
    When miner MINER mines 13 new blocks
    Then VN has scanned to at least height 35
    When I claim burn BURN_PROOF and spend it into account ACC using wallet daemon WALLET_D

    Then I wait for ACC on wallet daemon WALLET_D to have balance gte 900000

#  @serial
  @concurrent
  Scenario: Double claim base layer burn funds with wallet daemon. should fail
    Given a network with registered validator VN and wallet daemon WALLET_D

    When I create an account ACC via the wallet daemon WALLET_D

    When I burn 10T on wallet MINOTARI_WALLET to proof BURN_PROOF for wallet daemon WALLET_D

    # unfortunately have to wait for this to get into the mempool....
    Then there is 1 transaction in the mempool of BASE_NODE within 10 seconds
    When miner MINER mines 5 new blocks

    When I wait for proof BURN_PROOF to confirm on wallet MINOTARI_WALLET

    When miner MINER mines 13 new blocks
    Then VN has scanned to at least height 35

    When I claim burn BURN_PROOF and spend it into account ACC using wallet daemon WALLET_D
    When I claim burn BURN_PROOF and spend it into account ACC using wallet daemon WALLET_D, it fails

    # Then we check the balance
    Then I wait for ACC on wallet daemon WALLET_D to have balance gte 900000

  @concurrent
  Scenario: Auto claim burn - wallet daemon auto-submits claim when proof file is detected
    Given a network with registered validator VN and wallet daemon WALLET_D

    When I create an account ACC via the wallet daemon WALLET_D

    When I burn 10T on wallet MINOTARI_WALLET to proof BURN_PROOF for wallet daemon WALLET_D

    Then there is 1 transaction in the mempool of BASE_NODE within 10 seconds
    When miner MINER mines 5 new blocks

    When I wait for proof BURN_PROOF to confirm on wallet MINOTARI_WALLET
    When miner MINER mines 13 new blocks
    Then VN has scanned to at least height 35

    # Drop the burn proof file into the wallet daemon's burn_proof_dir.
    # The auto-claim service detects it, waits for the next epoch, then submits the claim.
    When I drop burn proof BURN_PROOF as a file for wallet daemon WALLET_D

    Then I wait for ACC on wallet daemon WALLET_D to have balance gte 900000
