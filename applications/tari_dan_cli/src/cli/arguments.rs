// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{env, path::PathBuf};

use anyhow::anyhow;
use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
    Subcommand,
};
use convert_case::{Case, Casing};

use crate::{
    cli::{
        commands::{create, new},
        config::{Config, TemplateRepository},
        util,
    },
    git::repository::GitRepository,
    loading,
};

const DEFAULT_DATA_FOLDER_NAME: &str = "tari_dan_cli";
const TEMPLATE_REPOS_FOLDER_NAME: &str = "template_repositories";
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
    dirs_next::data_dir()
        .unwrap_or_else(|| env::current_dir().unwrap())
        .join(DEFAULT_DATA_FOLDER_NAME)
}

fn default_target_dir() -> PathBuf {
    env::current_dir().unwrap()
}

fn default_config_file() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| env::current_dir().unwrap())
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
    let (key, value) = (splitted.first().unwrap(), splitted.get(1).unwrap());

    if !Config::is_override_key_valid(key) {
        return Err(format!("Override key invalid: {}", key));
    }

    Ok(ConfigOverride {
        key: key.to_string(),
        value: value.to_string(),
    })
}

pub fn project_name_parser(project_name: &str) -> Result<String, String> {
    Ok(project_name.to_case(Case::Snake))
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
    #[arg(short = 'c', long, value_name = "PATH", default_value = default_config_file().into_os_string()
    )]
    config_file_path: PathBuf,

    /// Config file overrides
    #[arg(short = 'e', long, value_name = "KEY=VALUE", value_parser = config_override_parser
    )]
    config_overrides: Vec<ConfigOverride>,
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
        #[arg(value_parser = project_name_parser)]
        name: String,

        /// (Optional) Selected project template (ID).
        /// It will be prompted if not set.
        #[arg(short = 't', long)]
        template: Option<String>,

        /// Target folder where the new project will be generated
        #[arg(long, value_name = "PATH", default_value = default_target_dir().into_os_string()
        )]
        target: PathBuf,
    },
    /// Creates a new Tari wasm template project
    New {
        /// Name of the project
        #[arg(value_parser = project_name_parser)]
        name: String,

        /// (Optional) Selected wasm template (ID).
        /// It will be prompted if not set.
        #[arg(short = 't', long)]
        template: Option<String>,

        /// Target folder where the new project will be generated.
        #[arg(long, value_name = "PATH", default_value = default_target_dir().into_os_string()
        )]
        target: PathBuf,
    },
}

impl Cli {
    async fn init_base_dir_and_config(&self) -> anyhow::Result<Config> {
        // make sure we have all the directories set up
        util::create_dir(&self.args.base_dir).await?;
        if !util::path_metadata(&self.args.config_file_path).await?.is_file() {
            return Err(anyhow!("Configuration file path is not pointing to a file!"));
        }
        util::create_dir(
            &self
                .args
                .config_file_path
                .parent()
                .ok_or(anyhow!("Can't find folder of configuration file!"))?
                .to_path_buf(),
        )
        .await?;

        // loading/creating config
        let mut config = if !util::file_exists(&self.args.config_file_path).await? {
            let cfg = Config::default();
            cfg.write_to_file(&self.args.config_file_path).await?;
            cfg
        } else {
            match Config::open(&self.args.config_file_path).await {
                Ok(cfg) => cfg,
                Err(error) => {
                    println!("Failed to open config file: {error:?}, creating default...");
                    let cfg = Config::default();
                    cfg.write_to_file(&self.args.config_file_path).await?;
                    cfg
                },
            }
        };

        // apply config overrides
        for config_override in &self.args.config_overrides {
            config.override_data(config_override.key.as_str(), config_override.value.as_str())?;
        }

        Ok(config)
    }

    async fn refresh_template_repository(&self, template_repo: &TemplateRepository) -> anyhow::Result<GitRepository> {
        util::create_dir(&self.args.base_dir.join(TEMPLATE_REPOS_FOLDER_NAME)).await?;
        let repo_url_splitted: Vec<&str> = template_repo.url.split("/").collect();
        let repo_name = repo_url_splitted
            .last()
            .ok_or(anyhow!("Failed to get repository name from URL!"))?;
        let repo_user = repo_url_splitted
            .get(repo_url_splitted.len() - 2)
            .ok_or(anyhow!("Failed to get repository owner from URL!"))?;
        let repo_folder_path = self
            .args
            .base_dir
            .join(TEMPLATE_REPOS_FOLDER_NAME)
            .join(repo_user)
            .join(repo_name);
        let mut repo = GitRepository::new(repo_folder_path.clone());

        match util::dir_exists(&repo_folder_path).await? {
            true => {
                repo.load()?;
                let current_branch = repo.current_branch_name()?;
                if current_branch != template_repo.branch {
                    repo.pull_changes(Some(template_repo.branch.clone()))?;
                } else {
                    repo.pull_changes(None)?;
                }
            },
            false => {
                repo.clone_and_checkout(template_repo.url.as_str(), template_repo.branch.as_str())?;
            },
        }

        Ok(repo)
    }

    pub async fn handle_command(&self) -> anyhow::Result<()> {
        // init config and dirs
        let config = loading!(
            "Init configuration and directories",
            self.init_base_dir_and_config().await
        )?;

        // refresh templates from provided repositories
        let project_template_repo = loading!(
            "Refresh project templates repository",
            self.refresh_template_repository(&config.project_template_repository)
                .await
        )?;
        let wasm_template_repo = loading!(
            "Refresh wasm templates repository",
            self.refresh_template_repository(&config.wasm_template_repository).await
        )?;

        match &self.command {
            Commands::Create { name, template, target } => {
                create::handle(
                    config,
                    project_template_repo,
                    name.as_str(),
                    template.as_ref(),
                    target.clone(),
                )
                .await
            },
            Commands::New { name, template, target } => {
                new::handle(
                    config,
                    wasm_template_repo,
                    name.as_ref(),
                    template.as_ref(),
                    target.clone(),
                )
                .await
            },
        }
    }
}
