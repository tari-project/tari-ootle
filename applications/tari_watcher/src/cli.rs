// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    env,
    path::{Path, PathBuf},
};

use clap::Parser;
use tari_common::configuration::Network;

use crate::{config::Config, constants::DEFAULT_VALIDATOR_DIR};

#[derive(Clone, Debug, Parser)]
pub struct Cli {
    #[clap(flatten)]
    pub common: CommonCli,
    #[clap(subcommand)]
    pub command: Commands,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }

    pub fn get_config_path(&self) -> PathBuf {
        self.relative_to_base_path(&self.common.config_path)
    }

    pub fn get_validator_node_base_dir(&self) -> PathBuf {
        abs_or_with_base_dir(&self.common.validator_dir)
    }

    pub fn get_base_path(&self) -> PathBuf {
        abs_or_with_base_dir(&self.common.base_dir)
    }

    fn relative_to_base_path<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            return path.to_path_buf();
        }
        self.get_base_path().join(path)
    }
}

fn abs_or_with_base_dir<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    if p.is_absolute() {
        return p.to_path_buf();
    }
    env::current_dir().expect("Failed to get current directory").join(p)
}

#[derive(Debug, Clone, clap::Args)]
pub struct CommonCli {
    #[clap(short = 'b', long, parse(from_os_str), default_value = "data/watcher/")]
    pub base_dir: PathBuf,
    #[clap(short = 'c', long, parse(from_os_str), default_value = "config.toml")]
    pub config_path: PathBuf,
    #[clap(short = 'v', long, parse(from_os_str), default_value = DEFAULT_VALIDATOR_DIR)]
    pub validator_dir: PathBuf,
    #[clap(short = 'n', long)]
    pub network: Option<Network>,
}

#[derive(Clone, Debug, clap::Subcommand)]
pub enum Commands {
    Init(InitArgs),
    Start(Overrides),
}

#[derive(Clone, Debug, clap::Args)]
pub struct InitArgs {
    #[clap(long)]
    /// Disable initial and auto registration of the validator node
    pub no_auto_register: bool,

    #[clap(long)]
    /// Disable auto restart of the validator node
    pub no_auto_restart: bool,

    #[clap(long, short = 'f')]
    pub force: bool,
}

impl InitArgs {
    pub fn apply(&self, config: &mut Config) {
        config.auto_register = !self.no_auto_register;
        config.auto_restart = !self.no_auto_restart;
    }
}

#[derive(Clone, Debug, clap::Args)]
pub struct Overrides {
    /// The path to the validator node binary (optional)
    #[clap(long)]
    pub vn_node_path: Option<PathBuf>,
}

impl Overrides {
    pub fn apply(&self, config: &mut Config) {
        if let Some(path) = self.vn_node_path.clone() {
            log::info!("Overriding validator node binary path to {:?}", path);
            config.validator_node_executable_path = path;
        }
    }
}
