# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@claim_fees
Feature: Claim Fees

  @serial @fixed
  Scenario: Claim validator fees
    Given a network with spec
  """
  validators:
    - name: VN
      fee_claim_account: VN_FEES
  walletds:
    - name: WALLET_D
      with_account: VN_FEES
  indexer:
    name: IDX
  """

    # Run some transactions to generate fees
    When I create an account ACC1 via the wallet daemon WALLET_D with 10 XTR
    When wallet daemon WALLET_D publishes the template "fees" using account ACC1
    Then I run up 550000 in fees using the wallet daemon WALLET_D and account ACC1

    # Progress to the next epoch
    When miner MINER mines 10 new blocks
    Then VN has scanned to at least height 27

    When I check the balance of VN_FEES on wallet daemon WALLET_D the amount is exactly 0

    # Claim fees into ACC2
    When I claim fees for validator VN into account VN_FEES using the wallet daemon WALLET_D

    # Check that there is a net gain
    When I check the balance of VN_FEES on wallet daemon WALLET_D the amount is at least 500000

  @serial
  Scenario: Prevent double claim of validator fees
    Given a network with spec
  """
  validators:
    - name: VN
      fee_claim_account: VN_FEES
  walletds:
    - name: WALLET_D
      with_account: VN_FEES
  indexer:
    name: IDX
  """

    # Run some transactions to generate fees
    When I create an account ACC1 via the wallet daemon WALLET_D with 10 XTR
    When wallet daemon WALLET_D publishes the template "fees" using account ACC1
    Then I run up 550000 in fees using the wallet daemon WALLET_D and account ACC1

    # Progress to the next epoch
    When miner MINER mines 10 new blocks
    Then VN has scanned to at least height 27

    # Can't claim fees with different account
    When I claim fees for validator VN into account ACC1 using the wallet daemon WALLET_D, it fails

    # Claim fees into VN_FEES
    When I check the balance of VN_FEES on wallet daemon WALLET_D the amount is exactly 0

    When I claim fees for validator VN into account VN_FEES using the wallet daemon WALLET_D
    # Fees that were run up are minimum 550000, in reality just over 600k (asserting this would be brittle though)
    When I check the balance of VN_FEES on wallet daemon WALLET_D the amount is at least 550000

    # Claim fees again, will succeed due to the fee from the previous claim but will only be a relatively small amount (so not 1_000_000)
    When I claim fees for validator VN into account VN_FEES using the wallet daemon WALLET_D
    When I check the balance of VN_FEES on wallet daemon WALLET_D the amount is at most 700000
