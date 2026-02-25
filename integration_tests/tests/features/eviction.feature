# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@eviction
Feature: Eviction scenarios

  @doit
  Scenario: Offline validator gets evicted
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
      - name: VN3
      - name: VN4
      - name: VN5
    walletds:
        - name: WALLET_D
    """

    When all validator nodes have started epoch 3


    Then I create an account ACC_1 via the wallet daemon WALLET_D with 10000 XTR
    Then I create an account ACC_2 via the wallet daemon WALLET_D with 10000 XTR

    Then I stop validator node VN5
    # Submit some transactions to speed up block production
    Then I create an account ACC_3 via the wallet daemon WALLET_D with 10000 XTR
    Then I create an account ACC_4 via the wallet daemon WALLET_D with 10000 XTR
    Then I create an account ACC_5 via the wallet daemon WALLET_D with 10000 XTR
    Then I create an account ACC_6 via the wallet daemon WALLET_D with 10000 XTR
    Then I create an account ACC_7 via the wallet daemon WALLET_D with 10000 XTR

    Then I wait for VN1 to list VN5 as evicted in EVICT_PROOF
    Then I submit the eviction proof EVICT_PROOF to MINOTARI_WALLET

    When miner MINER mines to the next epoch
    Then all validators have scanned to height 42
    When all validator nodes have started epoch 4
#    When miner MINER mines to the next epoch (TODO: make this step)
    When miner MINER mines 13 new blocks
    When all validator nodes have started epoch 5
    # TODO: this is flaky - the implementation of this step does not currently panic if this assertion fails
    Then validator VN5 is not a member of the current network according to BASE_NODE
