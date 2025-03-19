//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, path::PathBuf, str::FromStr};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(rename = "type")]
    pub database_type: AnyDatabaseType,
    pub rocks_db: RocksConfig,
    pub sqlite: SqliteConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RocksConfig {
    pub path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SqliteConfig {
    pub path: PathBuf,
}

impl SqliteConfig {
    /// Convert the SQLite configuration into a connection string
    ///
    /// # Panics
    /// Panics if the path is not valid UTF-8
    pub fn to_connection_string(&self) -> String {
        format!(
            "sqlite://{}",
            self.path.as_os_str().to_str().expect("SQLite path is not valid UTF-8")
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnyDatabaseType {
    Sqlite,
    Rocksdb,
}

impl FromStr for AnyDatabaseType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("rocksdb") {
            return Ok(AnyDatabaseType::Rocksdb);
        }
        if s.eq_ignore_ascii_case("sqlite") {
            return Ok(AnyDatabaseType::Sqlite);
        }
        Err(anyhow!("Invalid database type '{}'", s))
    }
}

impl Display for AnyDatabaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnyDatabaseType::Rocksdb => write!(f, "rocksdb"),
            AnyDatabaseType::Sqlite => write!(f, "sqlite"),
        }
    }
}
