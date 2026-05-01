//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use tari_template_lib_types::TemplateAddress;
use url::Url;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub sub_command: SubCommand,
    #[clap(flatten)]
    pub common: CommonArgs,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }
}

#[derive(Args, Debug)]
pub struct CommonArgs {
    #[clap(long, short = 'd', alias = "db", default_value = "data/tariswap-test-bench.sqlite")]
    pub db_path: PathBuf,
    #[clap(long, short = 'i', alias = "indexer", default_value = "http://localhost:18300")]
    pub indexer_url: Url,
    #[clap(long, alias = "faucet")]
    pub faucet_template: Option<TemplateAddress>,
    #[clap(long, alias = "swap")]
    pub swap_template: Option<TemplateAddress>,
}

#[derive(Subcommand, Debug)]
pub enum SubCommand {
    Run(RunArgs),
}

#[derive(Args, Debug)]
pub struct RunArgs {}
