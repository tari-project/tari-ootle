# Copyright 2026 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@hybrid_oracle
Feature: Hybrid epoch oracle

  Scenario: Indexers started with an outdated validator set follow the validators through a join and an exit
    # The validators' config schedules VN5 to join in epoch 4 and VN1 to exit in epoch 6. INDEXER starts with a
    # config that lists neither change, as an indexer released before them would.
    Given a hybrid oracle network with spec
      """
      validators:
        - name: VN1
          exit_epoch: 6
        - name: VN2
        - name: VN3
        - name: VN4
        - name: VN5
          registration_epoch: 3
          known_to_indexer: false
      """
    When I create an account ACC1 via the wallet daemon WALLET_D with 10000 XTR
    Then indexer INDEXER proves the committee of epoch 2 is VN1,VN2,VN3,VN4

    # VN5 has been running since the start, and syncs the state committed before it activates
    When miner MINER mines to the next epoch
    When miner MINER mines to the next epoch
    Then validator node VN5 reports consensus state Running within 180 seconds
    Then the validator node VN5 has started epoch 4
    When I create an account ACC2 via the wallet daemon WALLET_D with 10000 XTR
    When I wait for validator VN5 has leaf block height of at least 3 at epoch 4
    Then indexer INDEXER proves the committee of epoch 4 is VN1,VN2,VN3,VN4,VN5

    # VN1 stays in its committee up to and including epoch 5
    When miner MINER mines to the next epoch
    Then indexer INDEXER proves the committee of epoch 5 is VN1,VN2,VN3,VN4,VN5
    When miner MINER mines to the next epoch
    Then indexer INDEXER proves the committee of epoch 6 is VN2,VN3,VN4,VN5
    When I shut down validator node VN1
    When I create an account ACC3 via the wallet daemon WALLET_D with 10000 XTR
    When I wait for validator VN5 has leaf block height of at least 3 at epoch 6
    When miner MINER mines to the next epoch
    Then indexer INDEXER proves the committee of epoch 7 is VN2,VN3,VN4,VN5

    # A restart resumes from the config INDEXER adopted, which holds the exit it has applied
    When I stop indexer INDEXER
    When I start indexer INDEXER
    Then indexer INDEXER proves the committee of epoch 7 is VN2,VN3,VN4,VN5

    # A fresh indexer with the same outdated config replays every epoch under the validators' config
    When I start indexer LATE_INDEXER with the epoch oracle config of indexer INDEXER
    Then indexer LATE_INDEXER proves the committee of epoch 7 is VN2,VN3,VN4,VN5
