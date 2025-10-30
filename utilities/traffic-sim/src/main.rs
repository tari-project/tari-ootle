//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod coin;
mod sim;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::LevelFilter;
use tari_ootle_common_types::engine_types::published_template::PublishedTemplateAddress;
use tari_template_lib::models::{ResourceAddress, UtxoId};

use crate::sim::TrafficSim;

#[derive(Parser)]
#[command(name = "traffic-sim")]
#[command(about = "A CLI tool for simulating traffic between wallets in a swarm")]
struct Cli {
    #[arg(
        long,
        default_value = "http://localhost:8080/json_rpc",
        help = "URL of the swarm API"
    )]
    swarm_url: String,

    #[arg(
        short = 'x',
        long,
        default_value = "http://localhost:9000/json_rpc",
        help = "URL of the exchange wallet API"
    )]
    exchange_wallet_url: String,

    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    pub fn init() -> Self {
        Cli::parse()
    }
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long, default_value_t = 100000000, help = "Minimum transaction value")]
        min_value: u64,

        #[arg(long, default_value_t = 200000000, help = "Maximum transaction value")]
        max_value: u64,

        #[arg(short = 'r', long, help = "Resource address to use for transactions (optional)")]
        resource_address: Option<ResourceAddress>,

        #[arg(
            short = 'c',
            long,
            help = "Path to coin file. If not provided, resource_address must be set"
        )]
        coin_file: Option<PathBuf>,
    },
    Init {
        #[arg(short = 't', long, help = "Stable coin template address")]
        template_address: PublishedTemplateAddress,

        #[arg(short = 'o', long, help = "Coin file output path")]
        coin_file: PathBuf,
    },
    Setup {
        #[arg(short = 'c', long, help = "Path to coin file")]
        coin_file: PathBuf,
    },
    ListWallets,
    DecryptUtxos {
        #[arg(long, default_value_t = 0, help = "Minimum transaction value")]
        min_value: u64,

        #[arg(long, default_value_t = 300000000, help = "Maximum transaction value")]
        max_value: u64,
        #[arg(short = 'r', long, help = "Resource address of the resource to decrypt")]
        resource_address: ResourceAddress,

        #[arg(long, help = "Last UTXO ID processed")]
        last_id: Option<String>,

        #[arg(long, help = "Specific UTXO ID to decrypt")]
        specific_id: Option<String>,

        #[arg(long, alias = "csv", help = "Path to output CSV file with results")]
        csv_output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    let cli = Cli::init();

    match cli.command {
        Commands::Run {
            min_value,
            max_value,
            resource_address,
            coin_file,
        } => {
            if min_value >= max_value {
                return Err(anyhow::anyhow!("min-value must be less than max-value"));
            }
            let coin = coin_file.as_ref().map(read_coin_file).transpose()?;

            let mut sim = TrafficSim::new(cli.swarm_url, cli.exchange_wallet_url);
            let resource_address = coin
                .as_ref()
                .map(|c| c.resource_address)
                .or(resource_address)
                .ok_or_else(|| {
                    anyhow::anyhow!("Either resource_address or coin_file must be provided to run the simulation")
                })?;
            sim.run_simulation(resource_address, min_value, max_value).await?;
        },
        Commands::Init {
            template_address,
            coin_file,
        } => {
            let mut sim = TrafficSim::new(cli.swarm_url, cli.exchange_wallet_url);
            sim.setup_stablecoin(template_address, coin_file).await?;
        },
        Commands::Setup { coin_file } => {
            let coin = read_coin_file(coin_file)?;
            let mut sim = TrafficSim::new(cli.swarm_url, cli.exchange_wallet_url);
            sim.connect_to_wallets().await?;
            sim.setup_accounts().await?;
            sim.setup_wallet_funds(coin.component_address, coin.resource_address, coin.admin_badge)
                .await?;
        },
        Commands::ListWallets => {
            let mut sim = TrafficSim::new(cli.swarm_url, cli.exchange_wallet_url);
            sim.connect_to_wallets().await?;
            if sim.wallets().is_empty() {
                println!("No wallets found in swarm");
                return Ok(());
            }

            println!("Found {} wallets:", sim.wallets().len());
            for (i, wallet) in sim.wallets().iter().enumerate() {
                println!("{}) {}", i + 1, wallet.client.endpoint());
            }
        },
        Commands::DecryptUtxos {
            min_value,
            max_value,
            resource_address,
            last_id,
            specific_id,
            csv_output,
        } => {
            let mut sim = TrafficSim::new(cli.swarm_url, cli.exchange_wallet_url);
            sim.decrypt_utxos(
                min_value,
                max_value,
                resource_address,
                last_id
                    .as_ref()
                    .map(|id| UtxoId::from_hex(id).context("Invalid last_id"))
                    .transpose()?,
                specific_id
                    .as_ref()
                    .map(|id| UtxoId::from_hex(id).context("Invalid specific_id"))
                    .transpose()?,
                csv_output,
            )
            .await?;
        },
    }

    Ok(())
}

fn read_coin_file<P: AsRef<Path>>(path: P) -> Result<coin::Coin> {
    let file = std::fs::File::open(path).context("Failed to open coin file")?;
    let coin = serde_json::from_reader(file).context("Failed to parse coin file")?;
    Ok(coin)
}
