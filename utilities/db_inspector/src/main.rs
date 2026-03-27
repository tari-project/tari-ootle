//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::Context;

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

fn init_logging() {
    fern::Dispatch::new()
        .format(|out, message, record| {
            if record.level() == log::Level::Info {
                out.finish(format_args!("{}", message))
            } else {
                out.finish(format_args!(
                    "[{}][{}] {}",
                    humantime::format_rfc3339(std::time::SystemTime::now()),
                    record.level(),
                    message
                ))
            }
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()
        .expect("Failed to initialize logging");
}
