# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Consensus stall recovery via highest-QC probe

  @serial @stall_recovery
  Scenario: All committee members restart after consensus stalls before an epoch boundary
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
      - name: VN3
      - name: VN4
    """

    # Advance through one clean epoch boundary so there is a finalised checkpoint
    # behind the leaf, then commit a transaction mid-epoch so each VN's persisted
    # leaf lives in an epoch with no finalised checkpoint.
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 4
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 5
    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR

    # Stop the entire committee mid-epoch 5. No checkpoint for epoch 5 was produced
    # (the boundary block never finalised because we shut everyone down first).
    When I shut down validator node VN1
    When I shut down validator node VN2
    When I shut down validator node VN3
    When I shut down validator node VN4

    # Advance the base-layer oracle past epoch 5 while the committee is offline so
    # that on restart `oracle_epoch > leaf.epoch`.
    Then miner MINER mines to the next epoch

    # Bring all four back up. With the old logic this would unconditionally trigger
    # state-sync, which would fail because no committee member has a checkpoint for
    # epoch 5 to hand out. With the highest-QC probe, the nodes verify each other's
    # high QCs at epoch 5, conclude that consensus stalled there, suppress state-sync,
    # and resume consensus at their existing leaves.
    When I start validator node VN1
    When I start validator node VN2
    When I start validator node VN3
    When I start validator node VN4
    When validator nodes reconnect to each other

    Then validator node VN1 reports consensus state Running within 60 seconds
    Then validator node VN2 reports consensus state Running within 60 seconds
    Then validator node VN3 reports consensus state Running within 60 seconds
    Then validator node VN4 reports consensus state Running within 60 seconds

    # End-to-end confirmation: the recovered committee can finalise transactions
    # again. If consensus is healthy this commits.
    Then I create an account ACC_2 via the wallet daemon WALLETD with 10000 XTR
