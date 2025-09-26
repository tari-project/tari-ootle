# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Leader failure scenarios

  @serial
  Scenario: Leader failure with single committee
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
      - name: VN3
      - name: VN4
    """

    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR
    Then I wait for VN1 to have at least 5 blocks for the current epoch

    # Stop VN 4
    Then I stop validator node VN4

    # Transactions should still finalize
    Then I create an account ACC_2 via the wallet daemon WALLETD with 10000 XTR
    Then I create an account ACC_3 via the wallet daemon WALLETD with 10000 XTR
    Then I create an account ACC_4 via the wallet daemon WALLETD with 10000 XTR

  @serial @ignore
  Scenario: Leader failure with multiple committees
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
      - name: VN3
      - name: VN4
      - name: VN5
      - name: VN6
      - name: VN7
      - name: VN8
      - name: VN9
      - name: VN10
    """

    Then I create an account ACC_1 via the wallet daemon WALLETD with 10000 XTR
    Then I wait for VN1 to have at least 5 blocks for the current epoch

    # Stop VN 4
    When I stop validator node VN4

    Then I create an account ACC_2 via the wallet daemon WALLETD with 10000 XTR
    Then I create an account ACC_3 via the wallet daemon WALLETD with 10000 XTR
    Then I create an account ACC_4 via the wallet daemon WALLETD with 10000 XTR
    Then I create an account ACC_5 via the wallet daemon WALLETD with 10000 XTR

