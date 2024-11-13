use crate::cli::config::Config;

/// Handle create command.
/// It creates a new Tari template development project.
pub async fn handle(_config: Config, _name: &str) -> anyhow::Result<()> {
    Ok(())
}