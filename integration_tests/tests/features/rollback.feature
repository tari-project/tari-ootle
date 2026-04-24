# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Break-glass rollback to epoch checkpoint

  @serial @rollback
  Scenario: Rollback to a prior epoch and resume consensus
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
    """

    # After `network with spec`, validators are registered and running in some epoch N
    # (epoch 3 with the current devnet constants). We need at least one *completed* epoch
    # behind us so there is a checkpoint to roll back to — mine forward until the next
    # EndOfEpoch has been committed, then mine again so we are two epochs past the
    # checkpoint target. The checkpoint for epoch 4 will exist once VNs have moved past it.
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 4
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 5

    # Submit a transaction in the current (post-checkpoint) epoch. This is the committed
    # state the rollback must undo.
    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR
    Then validator node VN1 has committed transaction ACC_1
    Then validator node VN2 has committed transaction ACC_1

    # Roll back to the previous epoch's checkpoint. Target is derived from VN1's current
    # consensus epoch so the test doesn't hardcode the exact epoch number — it stays
    # correct if the network setup timing shifts by an epoch.
    When I issue a rollback directive to the previous epoch to all validators

    # Consensus should reach a post-rollback Running state. After release_on_hold the
    # state machine transitions OnHold -> Idle -> CheckSync -> Running and installs a
    # fresh genesis at the current L1 epoch using the truncated state merkle root.
    Then validator node VN1 reports consensus state Running
    Then validator node VN2 reports consensus state Running

    # The transaction that was committed in the rolled-back epoch must no longer have a
    # finalized execution — its block and block_transaction_executions record were
    # removed by rollback_delete_after_epoch.
    Then validator node VN1 has not committed transaction ACC_1
    Then validator node VN2 has not committed transaction ACC_1

    # The fresh genesis means the local chain is at height 0 (or a very small leading
    # height if the pacemaker already produced a dummy block by the time we sample).
    Then validator node VN1 reports consensus height at most 3
    Then validator node VN2 reports consensus height at most 3

    # Submit a fresh transaction after the rollback. This verifies that consensus isn't
    # just "Running" on paper but actually accepting, executing, and committing new
    # transactions on the post-rollback chain.
    Then I create an account ACC_2 via the wallet daemon WALLETD with 10000 XTR
    Then validator node VN1 has committed transaction ACC_2
    Then validator node VN2 has committed transaction ACC_2

    # Consensus must also cross the next epoch boundary — mine the next epoch and verify
    # both VNs follow L1. If the rollback left anything inconsistent in the epoch manager
    # bookkeeping this step is the one that catches it.
    Then miner MINER mines to the next epoch
    When all validator nodes have started epoch 6
