use crate::cli::config::Config;
use crate::cli::util;
use clap::builder::styling::AnsiColor;
use clap::builder::Styles;
use clap::{Parser, Subcommand};
use std::env;
use std::path::PathBuf;

const DEFAULT_DATA_FOLDER_NAME: &str = "tari_dan_cli";
const DEFAULT_CONFIG_FILE_NAME: &str = "tari.config.toml";

pub fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::BrightMagenta.on_default())
        .usage(AnsiColor::BrightMagenta.on_default())
        .literal(AnsiColor::BrightGreen.on_default())
        .placeholder(AnsiColor::BrightMagenta.on_default())
        .error(AnsiColor::BrightRed.on_default())
        .invalid(AnsiColor::BrightRed.on_default())
        .valid(AnsiColor::BrightGreen.on_default())
}

fn default_base_dir() -> PathBuf {
    dirs_next::data_dir().unwrap_or_else(|| env::current_dir().unwrap())
        .join(DEFAULT_DATA_FOLDER_NAME)
}

fn default_config_dir() -> PathBuf {
    dirs_next::config_dir().unwrap_or_else(|| env::current_dir().unwrap())
        .join(DEFAULT_DATA_FOLDER_NAME)
        .join(DEFAULT_CONFIG_FILE_NAME)
}

pub fn config_override_parser(config_override: &str) -> Result<ConfigOverride, String> {
    if config_override.is_empty() {
        return Err(String::from("Override cannot be empty!"));
    }

    let splitted: Vec<&str> = config_override.split("=").collect();
    if splitted.len() != 2 {
        return Err(String::from("Invalid override!"));
    }
    let (key, value) = (splitted.get(0).unwrap(), splitted.get(0).unwrap());

    if !Config::is_override_key_valid(*key) {
        return Err(String::from("Override key invalid!"));
    }

    Ok(ConfigOverride { key: "".to_string(), value: "".to_string() })
}

#[derive(Clone, Debug)]
pub struct ConfigOverride {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Parser, Debug)]
pub struct CommonArguments {
    /// Base directory, where all the CLI data will be saved
    #[arg(short = 'b', long, value_name = "PATH", default_value = default_base_dir().into_os_string()
    )]
    base_dir: PathBuf,

    /// Config file location
    #[arg(short = 'c', long, value_name = "PATH", default_value = default_config_dir().into_os_string()
    )]
    config: PathBuf,

    /// Config file overrides
    #[arg(short = 'e', long, value_name = "KEY=VALUE", value_parser = config_override_parser
    )]
    config_overrides: Vec<ConfigOverride>,
}

impl CommonArguments {
    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    pub fn config(&self) -> &PathBuf {
        &self.config
    }
}

#[derive(Clone, Parser)]
#[command(styles = cli_styles())]
#[command(
    version,
    about = "🚀 Tari DAN CLI 🚀",
    long_about = "🚀 Tari DAN CLI 🚀\nHelps you manage the flow of developing Tari templates."
)]
pub struct Cli {
    #[clap(flatten)]
    args: CommonArguments,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Subcommand)]
pub enum Commands {
    /// Creates a new Tari templates project
    Create {
        /// Name of the project
        #[command()]
        name: String,
    },
}

impl Cli {
    async fn init_base_dir_and_config(&self) -> anyhow::Result<Config> {
        util::create_dir(&self.args.base_dir).await?;

        let config = if !util::file_exists(&self.args.config).await? {
            let cfg = Config::default();
            cfg.write_to_file(&self.args.config).await?;
            cfg
        } else {
            match Config::open(&self.args.config).await {
                Ok(cfg) => cfg,
                Err(error) => {
                    println!("Failed to open config file: {error:?}, creating default...");
                    let cfg = Config::default();
                    cfg.write_to_file(&self.args.config).await?;
                    cfg
                }
            }
        };

        Ok(config)
    }
    pub async fn handle_command(&self) -> anyhow::Result<()> {
        self.init_base_dir_and_config().await?;
        match &self.command {
            Commands::Create { name } => {
                println!("Creating template");
                Ok(())
            }
        }
    }
}

