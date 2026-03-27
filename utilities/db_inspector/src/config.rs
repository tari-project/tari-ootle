//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Context;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub webserver: WebServerConfig,
    pub dbs: Vec<DatabaseConfig>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let config_str = std::fs::read_to_string(path).context("read_to_string")?;
        let config: Config = toml::from_str(&config_str).context("parse toml")?;
        Ok(config)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let config_str = toml::to_string(self).context("serialize to toml")?;
        std::fs::write(path, config_str).context("write to file")
    }

    pub fn get_database(&self, name: &str) -> Option<&DatabaseConfig> {
        self.dbs.iter().find(|db| db.name == name)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub secondary_path: PathBuf,
    pub name: String,
    pub group: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebServerConfig {
    pub bind_address: SocketAddr,
}
