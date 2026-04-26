use std::time::Duration;
use tokio::time::sleep;
use tari_base_node_client::BaseNodeClient;
use tari_ootle_common_types::Network;

pub async fn wait_for_validator_node_registration(
    base_node_client: &mut impl BaseNodeClient,
    validator_node: &str,
    max_attempts: u32,
    sleep_duration: Duration,
) -> anyhow::Result<()> {
    let mut attempts = 0;
    while attempts < max_attempts {
        let registered_validators = base_node_client.get_registered_validators().await?;
        if registered_validators.contains(&validator_node.to_string()) {
            return Ok(());
        }
        attempts += 1;
        sleep(sleep_duration).await;
    }
    bail!("Timed out waiting for validator node to pick up registration");
}