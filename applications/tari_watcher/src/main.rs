// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use anyhow::{anyhow, bail, Context};
use tari_shutdown::{Shutdown, ShutdownSignal};
use tokio::{fs, task::JoinHandle};
use transaction_worker::worker_loop;

use crate::{
    cli::{Cli, Commands},
    config::{get_base_config, Config},
    helpers::read_config_file,
    logger::init_logger,
    manager::{start_receivers, ManagerHandle, ProcessManager},
    process::create_pid_file,
    shutdown::exit_signal,
};

mod alerting;
mod cli;
mod config;
mod constants;
mod helpers;
mod logger;
mod manager;
mod minotari;
mod monitoring;
mod process;
mod shutdown;
mod transaction_worker;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::init();
    let config_path = cli.get_config_path();

    if config_path.is_dir() {
        bail!(
            "Config path '{}' does points to a directory, expected a file",
            config_path.display()
        );
    }

    init_logger()?;

    match cli.command {
        Commands::Init(ref args) => {
            if !args.force && config_path.exists() {
                bail!(
                    "Config file already exists at {} (use --force to overwrite)",
                    config_path.display()
                );
            }
            // set by default in CommonCli
            let parent = config_path.parent().context("parent path")?;
            fs::create_dir_all(parent).await?;

            let mut config = get_base_config(&cli)?;
            // optionally disables auto register
            args.apply(&mut config);

            let file = fs::File::create(&config_path)
                .await
                .with_context(|| anyhow!("Failed to open config path {}", config_path.display()))?;
            config.write(file).await.context("Writing config failed")?;

            log::info!("Config file created at {}", config_path.display());
        },
        Commands::Start(ref args) => {
            log::info!("Starting watcher using config {}", config_path.display());
            let mut cfg = read_config_file(config_path).await.context("read config file")?;

            // optionally override config values
            args.apply(&mut cfg);
            start(cfg).await?;
        },
    }

    Ok(())
}

async fn start(config: Config) -> anyhow::Result<()> {
    let mut shutdown = Shutdown::new();
    let signal = shutdown.to_signal().select(exit_signal()?);
    fs::create_dir_all(&config.base_dir)
        .await
        .context("create watcher base path")?;
    create_pid_file(config.base_dir.join("watcher.pid"), std::process::id()).await?;
    let Handlers { manager_handle, task } = spawn_manager(config.clone(), shutdown.to_signal()).await?;

    tokio::select! {
        _ = signal => {
            log::info!("Shutting down");
        },
        result = task => {
            result?;
            log::info!("Process manager exited");
        },
        Err(err) = worker_loop(config, manager_handle) => {
            log::error!("Registration loop exited with error {err}");
        },
    }

    shutdown.trigger();

    Ok(())
}

struct Handlers {
    manager_handle: ManagerHandle,
    task: JoinHandle<()>,
}

async fn spawn_manager(config: Config, shutdown: ShutdownSignal) -> anyhow::Result<Handlers> {
    let (manager, manager_handle) = ProcessManager::new(config, shutdown);
    let cr = manager.start_request_handler().await?;
    start_receivers(cr.rx_log, cr.rx_alert, cr.cfg_alert).await;

    Ok(Handlers {
        manager_handle,
        task: cr.task,
    })
}
