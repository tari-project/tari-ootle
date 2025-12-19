# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@substates
Feature: Substates

  Scenario: Transactions with DOWN local substates are rejected
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Create account
    When I create an account ACC via the wallet daemon WALLET_D with 10 XTR

    # Publish the "counter" template
    When wallet daemon WALLET_D publishes the template "counter" using account ACC

    # Create a new Counter component
    When I call function "new" on template "counter" using account ACC to pay fees via wallet daemon WALLET_D named "COUNTER_1"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER_1/components/Counter the method call "value" the result is "0"

    # Increase the counter and check the value
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER_1/components/Counter the method call "increase" named "TX1"
    When I invoke on wallet daemon WALLET_D on account ACC on component TX1/components/Counter the method call "value" the result is "1"

    # We should get an error if we se as inputs the same component version thas has already been downed from previous transactions
    # We can achieve this by reusing inputs from COUNTER_1 instead of the most recent TX1
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER_1/components/Counter the method call "increase" named "TX2", I expect it to fail with "Substate .*? is DOWN"

    # Check that the counter has NOT been increased by the previous erroneous transaction
    When I invoke on wallet daemon WALLET_D on account ACC on component TX1/components/Counter the method call "value" the result is "1"


