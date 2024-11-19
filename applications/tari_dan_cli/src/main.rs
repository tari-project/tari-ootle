use clap::Parser;

use crate::cli::arguments::Cli;

mod cli;
mod git;
mod templates;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().handle_command().await
}
