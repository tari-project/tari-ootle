Feature: State Sync
    As a validator node
    I want to register and sync with the network
    So that I can participate in the validation process

Scenario: New validator node registers and syncs
    Given a new validator node VN2
    When I send a registration transaction for VN2
    And I mine to the next epoch
    Then VN2 should be listed as registered
    And VN2 should be able to sync with the network