use crate::cli::arguments::Cli;
use clap::Parser;

mod cli;
mod git;
mod template;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().handle_command().await
}
