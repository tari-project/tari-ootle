//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Cli {
    #[clap(long, default_value = "data/db_inspector.toml", parse(from_os_str))]
    pub config_path: PathBuf,
    #[clap(long, short = 'n')]
    pub db_name: Option<String>,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }
}
