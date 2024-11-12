use crate::cli::arguments::Cli;
use clap::Parser;

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().handle_command().await
}
