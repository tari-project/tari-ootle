# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Catch-up sync within an epoch

  @serial @catch_up
  Scenario: A validator that missed blocks catches up from its committee
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
      - name: VN3
      - name: VN4
    """

    # A committed transaction while all four are up, so VN4 goes down in step with the rest.
    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR

    # VN4 misses everything from here. The other three are still a quorum (4 - floor(3/3) = 3), so
    # the chain keeps advancing: blocks for these transactions, plus a recovery block for every
    # height where VN4 was the leader.
    When I shut down validator node VN4
    Then I create an account ACC_2 via the wallet daemon WALLETD with 10000 XTR
    Then I create an account ACC_3 via the wallet daemon WALLETD with 10000 XTR

    # Back up in the same epoch, on the same data dir, with a leaf well below the committee's. No
    # epoch boundary was crossed, so there is no checkpoint to state-sync from: the only way back is
    # to request the missed blocks from the committee.
    When I start validator node VN4
    When validator nodes reconnect to each other
    Then validator node VN4 reports consensus state Running within 60 seconds
    Then validator node VN4 catches up to validator node VN1 within 240 seconds

    # And it is a full member again: a transaction after the catch-up still commits.
    Then I create an account ACC_4 via the wallet daemon WALLETD with 10000 XTR
