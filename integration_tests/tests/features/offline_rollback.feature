# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Offline break-glass rollback

  @serial @rollback
  Scenario: Rollback, restart, and continue producing blocks
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
    """

    # Advance past epoch 4 so both VNs have a persisted `EpochCheckpoint` to target.
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 4
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 5

    # Commit a transaction so there is state for the rollback to remove.
    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR

    # Shut down each VN (graceful axum shutdown releases the rocksdb LOCK) and run the
    # offline rollback tool on each data dir. Audit + history CF are asserted later.
    When I shut down validator node VN1
    When I apply an offline rollback to epoch 4 on validator node VN1
    When I shut down validator node VN2
    When I apply an offline rollback to epoch 4 on validator node VN2

    Then validator node VN1 has a rollback history entry at epoch 4
    Then validator node VN2 has a rollback history entry at epoch 4

    # Bring the validators back up. They need to find each other again — fresh p2p
    # ports on restart mean stale routing tables, so we explicitly re-wire peers.
    When I start validator node VN1
    When I start validator node VN2
    When validator nodes reconnect to each other

    Then validator node VN1 reports consensus state Running within 60 seconds
    Then validator node VN2 reports consensus state Running within 60 seconds

    # End-to-end confirmation: submit a new account-creation transaction after
    # rollback. If the chain is functional post-rollback (fresh genesis, pacemaker
    # producing blocks, proposals reaching quorum) this commits.
    Then I create an account ACC_2 via the wallet daemon WALLETD with 10000 XTR
