# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Break-glass rollback to epoch checkpoint

  @serial @rollback @ignore
  Scenario: Rollback to a prior epoch and resume consensus
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
    """

    # Drive the network past at least one epoch boundary so we have checkpoints to roll
    # back to, and some committed state to inspect.
    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR
    Then I wait for VN1 to have at least 5 blocks for the current epoch

    # Apply the rollback directive to every validator. With a 2-VN committee both must
    # accept for quorum to reform; the scenario reuses the same nonce-less directive flow
    # as the operator CLI.
    When I issue a rollback directive for target epoch 1 to all validators

    # Consensus should reach a post-rollback state. After release_on_hold the state
    # machine transitions OnHold -> Idle -> CheckSync -> Running; the existing genesis
    # path repopulates bookkeeping from a fresh genesis at the current L1 epoch.
    Then validator node VN1 reports consensus state Running
    Then validator node VN2 reports consensus state Running

    # The post-rollback consensus epoch should have moved back toward the target. It
    # won't necessarily equal target_epoch exactly — L1 advances in the background and
    # the catch-up path may have already produced a fresh genesis at the current L1
    # epoch — but it must not remain at whatever pre-rollback epoch we were at.
    Then validator node VN1 has rolled back past epoch 1
    Then validator node VN2 has rolled back past epoch 1
