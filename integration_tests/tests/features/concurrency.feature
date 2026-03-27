# Copyright 2024 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@concurrency
Feature: Concurrency

  Scenario: Concurrent calls to the Counter template
    Given a network with registered validator VN and wallet daemon WALLET_D

    # Create the sender account
    When I create an account ACC via the wallet daemon WALLET_D with 2 XTR

    # Publish the "counter" template
    When wallet daemon WALLET_D publishes the template "counter" using account ACC

    ##### Scenario
    # The initial value of the counter must be 0
    When I call function "new" on template "counter" using account ACC to pay fees via wallet daemon WALLET_D named "COUNTER"
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "0"

    # Send multiple concurrent transactions to increase the counter
    # Currently there is a lock bug where the subsequent transactions executed are being rejected, should be tested later after engine changes:
    # Reject(FailedToLockInputs("Failed to Write lock substate component_459d...4443c:1 due to conflict with existing Write lock"))
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "increase" concurrently 30 times

    # Check that the counter has been increased
    # Note: this is currently not working together with the previous test case when times > 1, only the first transaction is being executed properly
    When I invoke on wallet daemon WALLET_D on account ACC on component COUNTER/components/Counter the method call "value" the result is "30"
