// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use clap::Parser;

use crate::cli::arguments::Cli;

mod cli;
mod git;
mod templates;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().handle_command().await
}
