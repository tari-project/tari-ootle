use super::*;
use tari_base_node_client::BaseNodeClient;

Given("a new validator node VN2") {
    // Initialize VN2
}

When("I send a registration transaction for VN2") {
    // Send registration transaction
}

And("I mine to the next epoch") {
    // Mine to the next epoch
}

Then("VN2 should be listed as registered") {
    let base_node_client = // Initialize base node client
    let validator_node = "VN2";
    let max_attempts = 10;
    let sleep_duration = Duration::from_secs(10);
    wait_for_validator_node_registration(base_node_client, validator_node, max_attempts, sleep_duration).await?;
}

Then("VN2 should be able to sync with the network") {
    // Sync with the network
}