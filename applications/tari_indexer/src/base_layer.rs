use super::*;
use tari_base_node_client::BaseNodeClient;

pub async fn get_registered_validators<TClient: BaseNodeClient>(base_node_client: &mut TClient) -> anyhow::Result<Vec<String>> {
    let response = base_node_client.get_registered_validators().await?;
    Ok(response)
}