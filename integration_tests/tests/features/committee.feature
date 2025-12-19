# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@committee
Feature: Committee scenarios

  @serial
  Scenario: Template registration and invocation in a 2-VN committee
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
    """

    # Initialize indexer and connect wallet daemon
    When I create an account ACC via the wallet daemon WALLETD with 2000000 XTR
    When wallet daemon WALLETD publishes the template "counter" using account ACC
    Then the template "counter" is listed as registered by all validator nodes

  @serial
  Scenario: Template registration and invocation in a 4-VN committee
    Given a network with spec
    """
    validators:
      - name: VN1
      - name: VN2
      - name: VN3
      - name: VN4
    """

    # Register the "counter" template
    When I create an account ACC via the wallet daemon WALLETD with 2000000 XTR
    When wallet daemon WALLETD publishes the template "counter" using account ACC
    Then the template "counter" is listed as registered by all validator nodes


