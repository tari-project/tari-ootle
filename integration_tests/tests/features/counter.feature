# Copyright 2024 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrent
@counter
Feature: Counter template

  Scenario: Counter template registration and invocation once
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Create the sender account
    When I create an account ACC via the wallet daemon WALLET_D with 2 XTR

    # Publish the "counter" template
    When wallet daemon WALLET_D publishes the template "counter" using account ACC

    # The initial value of the counter must be 0
    When I call function "new" on template "counter" using account ACC to pay fees via wallet daemon WALLET_D named "COUNTER"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "0"

    # Increase the counter
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"

    # Check that the counter has been increased
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "1"


  Scenario: Counter template registration and invocation multiple times
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Create the sender account
    When I create an account ACC via the wallet daemon WALLET_D with 2 XTR

    # Publish the "counter" template
    When wallet daemon WALLET_D publishes the template "counter" using account ACC

    # The initial value of the counter must be 0
    When I call function "new" on template "counter" using account ACC to pay fees via wallet daemon WALLET_D named "COUNTER"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "0"

    # Increase and check the counter
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "1"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "2"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "3"
