//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    fmt,
    fmt::Display,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use tari_common::configuration::Network;
use tokio::{
    fs::File,
    io,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::cli::{Cli, Commands};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub base_dir: PathBuf,
    pub start_port: u16,
    pub network: Network,
    pub settings: Option<HashMap<String, String>>,
    pub webserver: WebserverConfig,
    #[serde(flatten)]
    pub processes: ProcessesConfig,
    #[serde(default)]
    pub skip_registration: bool,
    #[serde(default = "default_as_true")]
    pub auto_register_previous_templates: bool,
    pub public_ip: Option<IpAddr>,
}

fn default_as_true() -> bool {
    true
}

impl Config {
    pub async fn load_with_cli(cli: &Cli) -> anyhow::Result<Self> {
        let mut config = Self::load_from_file(cli.get_config_path()).await?;
        config.overrides_from_cli(cli);
        Ok(config)
    }

    pub async fn load_from_file<P: AsRef<Path>>(file: P) -> anyhow::Result<Self> {
        let mut file = File::open(file).await?;
        Self::load_from_reader(&mut file).await
    }

    pub async fn load_from_reader<R: io::AsyncRead + Unpin>(reader: &mut R) -> anyhow::Result<Self> {
        let mut s = String::new();
        reader.read_to_string(&mut s).await?;
        let config = toml::from_str(&s)?;
        Ok(config)
    }

    pub(crate) async fn write<W: io::AsyncWrite + Unpin>(&self, mut writer: W) -> anyhow::Result<()> {
        let toml = toml::to_string_pretty(self)?;
        writer.write_all(toml.as_bytes()).await?;
        Ok(())
    }

    pub fn get_public_ip(&self) -> IpAddr {
        self.public_ip.unwrap_or_else(|| [127, 0, 0, 1].into())
    }

    fn overrides_from_cli(&mut self, cli: &Cli) {
        if let Commands::Start(ref overrides) = cli.command {
            self.skip_registration = overrides.skip_registration;
        }
        if let Some(ref base_dir) = cli.common.base_dir {
            self.base_dir.clone_from(base_dir);
        }
        if let Some(network) = cli.common.network {
            self.network = network;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebserverConfig {
    pub bind_address: SocketAddr,
}

impl Default for WebserverConfig {
    fn default() -> Self {
        Self {
            bind_address: SocketAddr::from(([127, 0, 0, 1], 8080)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessesConfig {
    /// If true, the executables will be compiled even if the target binary file exists
    pub force_compile: bool,
    pub instances: Vec<InstanceConfig>,
    pub executables: Vec<ExecutableConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub use_execution_for: Option<InstanceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub base_path: Option<PathBuf>,
    pub instance_type: InstanceType,
    pub num_instances: u32,
    #[serde(alias = "extra_args")]
    pub settings: HashMap<String, String>,
}

impl InstanceConfig {
    pub fn new(instance_type: InstanceType) -> Self {
        Self {
            name: instance_type.to_string(),
            use_execution_for: None,
            base_path: None,
            instance_type,
            num_instances: 1,
            settings: HashMap::new(),
        }
    }

    pub fn with_name<S: Into<String>>(mut self, name: S) -> Self {
        self.name = name.into();
        self
    }

    pub fn use_execution_for(mut self, instance_type: InstanceType) -> Self {
        self.use_execution_for = Some(instance_type);
        self
    }

    pub fn with_base_path_override<P: Into<PathBuf>>(mut self, base_path: P) -> Self {
        self.base_path = Some(base_path.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_setting<K: Into<String>, V: ToString>(mut self, key: K, value: V) -> Self {
        self.settings.insert(key.into(), value.to_string());
        self
    }

    pub fn with_num_instances(mut self, num_instances: u32) -> Self {
        self.num_instances = num_instances;
        self
    }

    pub fn execution_instance_type(&self) -> InstanceType {
        self.use_execution_for.unwrap_or(self.instance_type)
    }

    pub fn instance_name(&self, index: u32) -> String {
        format!("{}-#{:02}", self.name, index)
    }

    pub fn base_path_override(&self) -> Option<&PathBuf> {
        self.base_path.as_ref()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum InstanceType {
    MinoTariNode,
    MinoTariConsoleWallet,
    MinoTariMiner,
    TariValidatorNode,
    TariIndexer,
    TariWalletDaemon,
    TariSignalingServer,
    TariWalletDaemonCreateKey,
}

impl InstanceType {
    pub fn is_base_layer_node(self) -> bool {
        matches!(self, InstanceType::MinoTariNode | InstanceType::MinoTariConsoleWallet)
    }

    pub fn is_tari_node(self) -> bool {
        matches!(
            self,
            InstanceType::TariValidatorNode | InstanceType::TariIndexer | InstanceType::TariWalletDaemon
        )
    }

    pub fn is_miner(self) -> bool {
        matches!(self, InstanceType::MinoTariMiner)
    }

    pub fn is_validator(self) -> bool {
        matches!(self, InstanceType::TariValidatorNode)
    }

    pub fn is_wallet_daemon_create_key(self) -> bool {
        matches!(self, InstanceType::TariWalletDaemonCreateKey)
    }
}

impl Display for InstanceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableConfig {
    pub instance_type: InstanceType,
    pub execuable_path: Option<PathBuf>,
    pub compile: Option<CompileConfig>,
    pub env: Vec<(String, String)>,
}

impl ExecutableConfig {
    pub fn get_executable_path(&self) -> Option<PathBuf> {
        self.execuable_path
            .clone()
            .or_else(|| self.compile.as_ref().map(|c| c.target_dir().join(&c.package_name)))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompileConfig {
    pub working_dir: Option<PathBuf>,
    pub package_name: String,
    pub target_dir: Option<PathBuf>,
}

impl CompileConfig {
    pub fn target_dir(&self) -> PathBuf {
        self.target_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("target/release"))
    }

    pub fn working_dir(&self) -> PathBuf {
        self.working_dir.clone().unwrap_or_else(|| PathBuf::from("."))
    }
}
