//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::Context;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::config::{Config, DatabaseConfig, WebServerConfig};

mod cli;
mod config;
mod helpers;
mod webserver;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();
    let cli = cli::Cli::init();

    let config = if cli.config_path.exists() {
        log::info!("Config file found at: {}", cli.config_path.display());
        Config::load(&cli.config_path).context("load config")?
    } else {
        log::info!("Config file not found at: {}", cli.config_path.display());
        let config = Config {
            webserver: WebServerConfig {
                bind_address: ([127, 0, 0, 1], 9090).into(),
            },
            dbs: vec![DatabaseConfig {
                path: "data/swarm/processes/validator-node-00/localnet/data/validator_node/rocksdb".into(),
                secondary_path: "data/db_inspector/secondaries/vn0".into(),
                name: "VN0".to_string(),
                group: None,
            }],
        };
        config.save(&cli.config_path)?;
        config
    };

    let interrupt = async { tokio::signal::ctrl_c().await.unwrap() };
    webserver::spawn(config, interrupt).await??;

    Ok(())
}

/// Current UTC time in RFC 3339, with fractional seconds only when they are non-zero.
fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc();
    // Rfc3339 formatting fails only for years outside 0..=9999.
    now.format(&Rfc3339).unwrap_or_else(|_| now.to_string())
}

fn init_logging() {
    fern::Dispatch::new()
        .format(|out, message, record| {
            if record.level() == log::Level::Info {
                out.finish(format_args!("{}", message))
            } else {
                out.finish(format_args!("[{}][{}] {}", now_rfc3339(), record.level(), message))
            }
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()
        .expect("Failed to initialize logging");
}
