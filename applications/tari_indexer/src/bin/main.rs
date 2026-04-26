use super::*;
use tari_base_node_client::BaseNodeClient;

#[tokio::main]
async fn main() {
    let mut base_node_client = // Initialize base node client
    let validator_node = "VN2";
    let max_attempts = 10;
    let sleep_duration = Duration::from_secs(10);
    wait_for_validator_node_registration(&mut base_node_client, validator_node, max_attempts, sleep_duration).await?;
}