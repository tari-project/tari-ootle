//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    io::BufReader as StdBufReader,
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use anyhow::{Context, anyhow};
use indexmap::IndexMap;
use log::{debug, info};
use slug::slugify;
use tari_ootle_wallet_sdk::Network;
use tokio::{
    fs,
    fs::File,
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    task,
    time::sleep,
};

use super::InstanceId;

/// Written to the swarm directory when there is no constants file there. Everything is commented
/// out, so it changes nothing until a line is uncommented, and it lists what may be changed without
/// having to go and look.
const CONSENSUS_CONSTANTS_TEMPLATE: &str = r#"# Consensus constants for this swarm, read once at
# start-up by every node in it. Only LocalNet reads this file at all. Uncomment a line to change it
# and restart the swarm; anything left commented keeps the network's own value.
#
# Every value below is the one this network already uses, so uncommenting a line as it stands changes
# nothing. The number of committees is the registered validator count divided by the committee size,
# so a smaller committee splits the shard space across fewer nodes.
# committee_size_per_shard_group = 7

# pacemaker_block_time_secs = 10
# base_layer_confirmations = 3
# missed_proposal_suspend_threshold = 5
# probation_base_votes = 5
# probation_base_blocks = 100
# probation_max_backoff_exp = 6
# max_transaction_validity_epochs = 2160
"#;
use crate::{
    config::{InstanceConfig, InstanceType},
    logger::FORWARDED_TARGET,
    process_definitions::{
        CLAIM_KEY_FILE_NAME,
        CLAIM_PUBLIC_KEY_SETTINGS_KEY,
        CONSENSUS_CONSTANTS_FILE_NAME,
        ProcessContext,
        get_definition,
    },
    process_manager::{
        AllocatedPorts,
        IndexerProcess,
        Instance,
        WalletDaemonProcess,
        executables::{Executable, Executables},
        port_allocator::PortAllocator,
        processes::{MinoTariMinerProcess, MinoTariNodeProcess, MinoTariWalletProcess, ValidatorNodeProcess},
    },
};

pub struct InstanceManager {
    base_path: PathBuf,
    config: Vec<InstanceConfig>,
    global_settings: HashMap<String, String>,
    network: Network,
    minotari_nodes: IndexMap<InstanceId, MinoTariNodeProcess>,
    minotari_wallets: IndexMap<InstanceId, MinoTariWalletProcess>,
    minotari_miners: IndexMap<InstanceId, MinoTariMinerProcess>,
    validator_nodes: IndexMap<InstanceId, ValidatorNodeProcess>,
    indexers: IndexMap<InstanceId, IndexerProcess>,
    wallet_daemons: IndexMap<InstanceId, WalletDaemonProcess>,
    port_allocator: PortAllocator,
    instance_id: InstanceId,
}

impl InstanceManager {
    pub fn new(
        base_path: PathBuf,
        network: Network,
        global_settings: HashMap<String, String>,
        config: Vec<InstanceConfig>,
        start_port: u16,
    ) -> Self {
        Self {
            base_path,
            port_allocator: PortAllocator::new(start_port, &config),
            config,
            network,
            global_settings,
            minotari_nodes: IndexMap::new(),
            minotari_wallets: IndexMap::new(),
            minotari_miners: IndexMap::new(),
            validator_nodes: IndexMap::new(),
            indexers: IndexMap::new(),
            wallet_daemons: IndexMap::new(),
            instance_id: 0,
        }
    }

    /// Fork all defined processes in order
    pub async fn fork_all(&mut self, executables: Executables<'_>) -> anyhow::Result<()> {
        self.write_consensus_constants_template().await?;
        for mut instance in self.config.clone() {
            let executable = executables.get(instance.execution_instance_type()).ok_or_else(|| {
                anyhow!(
                    "No executable found for instance type '{}'. This is a bug in the configuration",
                    instance.execution_instance_type()
                )
            })?;

            // The key outlives a restart, and creating it again fails because the account name is taken
            if instance.instance_type.is_wallet_daemon_create_key() &&
                fs::try_exists(self.base_path.join("processes").join(CLAIM_KEY_FILE_NAME))
                    .await
                    .unwrap_or(false)
            {
                info!("Claim key already exists, skipping {}", instance.name);
                continue;
            }

            let mut settings = self.global_settings.clone();
            settings.extend(instance.settings.drain());
            for i in 0..instance.num_instances {
                let name = instance.instance_name(i);
                let instance_id = self
                    .fork_new(
                        executable,
                        instance.instance_type,
                        name.clone(),
                        instance.base_path_override().cloned(),
                        instance.envs.clone(),
                        settings.clone(),
                    )
                    .await?;
                if instance.run_to_completion {
                    info!("Waiting for {name} to complete before starting further instances");
                    let status = self.wait(instance_id).await?;
                    if !status.success() {
                        return Err(anyhow!("{name} did not complete successfully: {status}"));
                    }
                }
            }
        }
        Ok(())
    }

    /// Writes a commented consensus constants file into the swarm directory if there is not one
    /// already. Every node forked here is pointed at it, so a devnet is retuned by uncommenting a
    /// line and restarting rather than by finding out where the setting lives.
    async fn write_consensus_constants_template(&self) -> anyhow::Result<()> {
        let path = self.base_path.join(CONSENSUS_CONSTANTS_FILE_NAME);
        if fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(());
        }

        fs::create_dir_all(&self.base_path)
            .await
            .context("create_dir_all for the consensus constants file")?;
        fs::write(&path, CONSENSUS_CONSTANTS_TEMPLATE)
            .await
            .context("write the consensus constants file")?;
        log::info!("📝 Wrote consensus constants file {}", path.display());
        Ok(())
    }

    /// Forks an instance of `instance_type` that the config does not list. It starts with the settings
    /// and envs of the first configured instance of that type, so it finds the rest of the swarm the
    /// way that instance does, and `settings` override any of them.
    /// With no `instance_name`, the instance takes the name the config gives its next instance of
    /// that type, so it reuses that process directory (and keys) when added again after a restart,
    /// and raising the entry's `num_instances` forks it again.
    pub async fn create_instance(
        &mut self,
        executable: &Executable,
        instance_type: InstanceType,
        instance_name: Option<String>,
        settings: HashMap<String, String>,
    ) -> anyhow::Result<InstanceId> {
        let instance_name = match instance_name {
            Some(name) => name,
            None => self.next_config_name(instance_type),
        };
        if self.is_name_taken(&instance_name) {
            return Err(anyhow!(
                "Instance with name '{instance_name}' already exists. Please choose a different name"
            ));
        }

        let mut new_settings = self.global_settings.clone();
        let mut envs = Vec::new();
        if let Some(config) = self.config.iter().find(|c| c.instance_type == instance_type) {
            new_settings.extend(config.settings.clone());
            envs.clone_from(&config.envs);
        }
        new_settings.extend(settings);

        self.fork_new(executable, instance_type, instance_name, None, envs, new_settings)
            .await
    }

    /// Names that slugify alike share a process directory, so they count as the same name
    fn is_name_taken(&self, name: &str) -> bool {
        let slug = slugify(name);
        self.instances().any(|i| slugify(i.name()) == slug)
    }

    /// The first name, in the config's naming of `instance_type`, that no instance holds.
    fn next_config_name(&self, instance_type: InstanceType) -> String {
        let config = self
            .config
            .iter()
            .find(|c| c.instance_type == instance_type)
            .cloned()
            .unwrap_or_else(|| InstanceConfig::new(instance_type));
        let mut index = 0;
        loop {
            let name = config.instance_name(index);
            if !self.is_name_taken(&name) {
                return name;
            }
            index += 1;
        }
    }

    pub async fn fork_new(
        &mut self,
        executable: &Executable,
        instance_type: InstanceType,
        instance_name: String,
        base_path_override: Option<PathBuf>,
        envs: Vec<(String, String)>,
        settings: HashMap<String, String>,
    ) -> anyhow::Result<InstanceId> {
        let instance_id = self.next_instance_id();
        self.fork(
            instance_id,
            executable,
            instance_type,
            instance_name,
            base_path_override,
            envs,
            settings,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_lines)]
    async fn fork(
        &mut self,
        instance_id: InstanceId,
        executable: &Executable,
        instance_type: InstanceType,
        instance_name: String,
        base_path_override: Option<PathBuf>,
        instance_envs: Vec<(String, String)>,
        mut instance_settings: HashMap<String, String>,
        ports: Option<AllocatedPorts>,
    ) -> anyhow::Result<InstanceId> {
        let listen_ip = instance_settings
            .get("listen_ip")
            .map(|s| s.parse())
            .transpose()
            .context("Failed to parse listen_ip arg")?
            .unwrap_or_else(|| IpAddr::V4(Ipv4Addr::from([127, 0, 0, 1])));
        let definition = get_definition(instance_type);

        log::info!(
            "🚀 Starting {} (id: {}, exec path: {}, listen_ip: {})",
            instance_type,
            instance_id,
            executable.path.display(),
            listen_ip
        );

        let mut allocated_ports = ports.unwrap_or_else(|| self.port_allocator.create(instance_type));

        let processes_path = self.base_path.join("processes");
        let base_path = match base_path_override {
            Some(base_path) => {
                if base_path.is_absolute() {
                    base_path
                } else {
                    processes_path.join(base_path)
                }
            },
            None => processes_path.join(slugify(&instance_name)),
        };
        fs::create_dir_all(&base_path).await.context("create_dir_all in fork")?;

        // Special handling to set the claim public key if we find a file containing one
        if instance_type.is_validator() && !instance_settings.contains_key(CLAIM_PUBLIC_KEY_SETTINGS_KEY) {
            let claim_public_key_file = processes_path.join(CLAIM_KEY_FILE_NAME);
            if claim_public_key_file.exists() {
                let file = File::open(&claim_public_key_file)
                    .await
                    .context("Failed to open claim public key file")?;
                let file = file.into_std().await;
                let reader = StdBufReader::new(file);
                let claim_data = serde_json::from_reader::<_, serde_json::Value>(reader)
                    .context("Failed to read claim public key file")?;
                let claim_public_key = claim_data
                    .get("account_public_key")
                    .and_then(|pk| pk.as_str())
                    .ok_or_else(|| anyhow!("Failed to extract public key from claim public key file: {claim_data}"))?;
                info!("Setting claim public key to {}", claim_public_key);
                instance_settings.insert(CLAIM_PUBLIC_KEY_SETTINGS_KEY.to_string(), claim_public_key.to_string());
            }
        }

        let context = ProcessContext::new(
            instance_id,
            &executable.path,
            &instance_envs,
            base_path.clone(),
            processes_path,
            self.base_path.clone(),
            self.network,
            listen_ip,
            &mut allocated_ports,
            self,
            &instance_settings,
        );
        if !context.bin().exists() {
            return Err(anyhow::anyhow!(
                "{} binary not found at {}",
                instance_type,
                context.bin().display()
            ));
        }

        let mut command = definition.get_command(context).await.context("get_command")?;
        let stdout_log_path = base_path.join("stdout.log");
        let stderr_log_path = base_path.join("stderr.log");
        command
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Any attempt to use stdin will fail immediately
            .stdin(Stdio::null());
        debug!("Command: {:?}", command);
        let mut child = command.spawn().with_context(|| format!("spawn {instance_type}"))?;

        self.port_allocator.register(instance_id, allocated_ports.clone());

        if let Some(stdout) = child.stdout.take() {
            forward_logs(stdout_log_path, stdout, instance_name.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            forward_logs(stderr_log_path, stderr, instance_name.clone());
        }

        let mut instance = Instance::new_started(
            instance_id,
            instance_name,
            instance_type,
            child,
            allocated_ports,
            // This saves us from having to join the network string to the path all over the place, since everything we
            // want is under {base_dir}/{network}
            base_path.join(self.network.to_string()),
            base_path,
            instance_envs,
            instance_settings,
        );

        // Wait for base layer nodes to start
        if instance_type.is_base_layer_node() {
            sleep(Duration::from_secs(2)).await;
            instance.check_running().context("Failed to start instance")?;
        }

        log::info!(
            "🟢 Started {} (id: {}, path: {}, pid: {:?})",
            instance_type,
            instance_id,
            executable.path.display(),
            instance.child().id()
        );

        match instance_type {
            InstanceType::MinoTariNode => {
                self.minotari_nodes
                    .insert_sorted(instance_id, MinoTariNodeProcess::new(instance));
            },
            InstanceType::MinoTariConsoleWallet => {
                self.minotari_wallets
                    .insert_sorted(instance_id, MinoTariWalletProcess::new(instance));
            },
            InstanceType::MinoTariMiner => {
                self.minotari_miners
                    .insert_sorted(instance_id, MinoTariMinerProcess::new(instance));
            },
            InstanceType::TariValidatorNode => {
                self.validator_nodes
                    .insert_sorted(instance_id, ValidatorNodeProcess::new(instance));
            },
            InstanceType::TariIndexer => {
                self.indexers.insert_sorted(instance_id, IndexerProcess::new(instance));
            },
            InstanceType::TariWalletDaemon => {
                self.wallet_daemons
                    .insert_sorted(instance_id, WalletDaemonProcess::new(instance));
            },
            InstanceType::TariWalletDaemonCreateKey => {
                self.wallet_daemons
                    .insert_sorted(instance_id, WalletDaemonProcess::new(instance));
            },
        }

        // Wait a bit after starting the instance
        sleep(definition.after_start_delay()).await;

        Ok(instance_id)
    }

    pub fn minotari_nodes(&self) -> impl Iterator<Item = &MinoTariNodeProcess> + Sized {
        self.minotari_nodes.values()
    }

    pub fn minotari_wallets(&self) -> impl Iterator<Item = &MinoTariWalletProcess> + Sized {
        self.minotari_wallets.values()
    }

    pub fn validator_nodes(&self) -> impl Iterator<Item = &ValidatorNodeProcess> + Sized {
        self.validator_nodes.values()
    }

    pub fn num_instances(&self) -> usize {
        self.instances().count()
    }

    pub fn num_validator_nodes(&self) -> usize {
        self.validator_nodes.len()
    }

    pub fn validator_nodes_mut(&mut self) -> impl Iterator<Item = &mut ValidatorNodeProcess> + Sized {
        self.validator_nodes.values_mut()
    }

    // pub fn minotari_miners(&self) -> impl Iterator<Item = &MinoTariMinerProcess> + Sized {
    //     self.minotari_miners.values()
    // }

    pub fn indexers(&self) -> impl Iterator<Item = &IndexerProcess> + Sized {
        self.indexers.values()
    }

    // pub fn wallet_daemons(&self) -> impl Iterator<Item = &WalletDaemonProcess> + Sized {
    //     self.wallet_daemons.values()
    // }

    /// Returns the long-running wallet daemon with this id. The short-lived key-creating run shares the
    /// `wallet_daemons` collection but serves no JSON-RPC, so it is never a usable daemon.
    pub fn get_wallet_daemon(&self, id: InstanceId) -> Option<&WalletDaemonProcess> {
        self.wallet_daemons
            .get(&id)
            .filter(|daemon| daemon.instance().instance_type().is_wallet_daemon())
    }

    pub fn get_instance_mut(&mut self, id: InstanceId) -> Option<&mut Instance> {
        self.instances_mut().find(|i| i.id() == id)
    }

    pub async fn wait(&mut self, id: InstanceId) -> anyhow::Result<ExitStatus> {
        let instance = self.get_instance_mut(id).ok_or_else(|| anyhow!("Instance not found"))?;
        let status = instance.child_mut().wait().await?;
        Ok(status)
    }

    pub async fn start_instance(&mut self, id: InstanceId, executable: &Executable) -> anyhow::Result<()> {
        let instance = self
            .instances()
            .find(|i| i.id() == id)
            .ok_or_else(|| anyhow!("Instance not found"))?;

        let instance_type = instance.instance_type();
        let instance_name = instance.name().to_string();
        let settings = instance.settings().clone();
        let ports = instance.allocated_ports().clone();
        let envs = instance.envs().to_vec();

        // This will just overwrite the previous instance
        self.fork(
            id,
            executable,
            instance_type,
            instance_name,
            None,
            envs,
            settings,
            Some(ports),
        )
        .await?;

        Ok(())
    }

    pub async fn stop_instance(&mut self, id: InstanceId) -> anyhow::Result<()> {
        let instance = self
            .instances_mut()
            .find(|i| i.id() == id)
            .ok_or_else(|| anyhow!("Instance not found"))?;

        info!("🛑 Stopping {} (id: {})", instance.instance_type(), instance.id());
        instance.terminate().await?;
        instance.check_running()?;
        Ok(())
    }

    pub async fn delete_instance_data(&mut self, id: InstanceId) -> anyhow::Result<()> {
        let instance = self
            .instances_mut()
            .find(|i| i.id() == id)
            .ok_or_else(|| anyhow!("Instance not found"))?;

        let definition = get_definition(instance.instance_type());

        if let Some(data_path) = definition.get_relative_data_path() {
            let path = instance.base_path().join(data_path);
            info!(
                "Deleting data directory for instance {}: {}",
                instance.name(),
                path.display()
            );
            if let Err(err) = fs::remove_dir_all(path).await &&
                err.kind() != std::io::ErrorKind::NotFound
            {
                return Err(err.into());
            }
        }
        Ok(())
    }

    pub fn remove_instance(&mut self, id: InstanceId) -> anyhow::Result<()> {
        let instance = self
            .instances()
            .find(|i| i.id() == id)
            .ok_or_else(|| anyhow!("Instance not found"))?;

        match instance.instance_type() {
            InstanceType::MinoTariNode => {
                self.minotari_nodes.shift_remove(&id);
            },
            InstanceType::MinoTariConsoleWallet => {
                self.minotari_wallets.shift_remove(&id);
            },
            InstanceType::MinoTariMiner => {
                self.minotari_miners.shift_remove(&id);
            },
            InstanceType::TariValidatorNode => {
                self.validator_nodes.shift_remove(&id);
            },
            InstanceType::TariIndexer => {
                self.indexers.shift_remove(&id);
            },
            InstanceType::TariWalletDaemon => {
                self.wallet_daemons.shift_remove(&id);
            },
            InstanceType::TariWalletDaemonCreateKey => {
                self.wallet_daemons.shift_remove(&id);
            },
        }

        // Remove allocated any ports for instance
        self.port_allocator.unregister(id);

        Ok(())
    }

    pub fn instances_mut(&mut self) -> impl Iterator<Item = &mut Instance> {
        self.minotari_nodes
            .values_mut()
            .map(|x| x.instance_mut())
            .chain(self.minotari_wallets.values_mut().map(|x| x.instance_mut()))
            .chain(self.minotari_miners.values_mut().map(|x| x.instance_mut()))
            .chain(self.validator_nodes.values_mut().map(|x| x.instance_mut()))
            .chain(self.indexers.values_mut().map(|x| x.instance_mut()))
            .chain(self.wallet_daemons.values_mut().map(|x| x.instance_mut()))
    }

    pub fn instances(&self) -> impl Iterator<Item = &Instance> {
        self.minotari_nodes
            .values()
            .map(|x| x.instance())
            .chain(self.minotari_wallets.values().map(|x| x.instance()))
            .chain(self.minotari_miners.values().map(|x| x.instance()))
            .chain(self.validator_nodes.values().map(|x| x.instance()))
            .chain(self.indexers.values().map(|x| x.instance()))
            .chain(self.wallet_daemons.values().map(|x| x.instance()))
    }

    /// Apply updated instance configs from a reloaded config file.
    /// Updates settings and envs for matching instances and marks them as config dirty
    /// so that the user knows to restart them.
    pub fn apply_config_update(
        &mut self,
        new_global_settings: HashMap<String, String>,
        new_instance_configs: &[InstanceConfig],
    ) {
        for instance_config in new_instance_configs {
            for i in 0..instance_config.num_instances {
                let expected_name = instance_config.instance_name(i);
                let mut new_settings = new_global_settings.clone();
                new_settings.extend(instance_config.settings.clone());

                if let Some(instance) = self.instances_mut().find(|inst| inst.name() == expected_name) {
                    if instance.instance_type().is_validator() &&
                        !new_settings.contains_key(CLAIM_PUBLIC_KEY_SETTINGS_KEY) &&
                        let Some(claim_key) = instance.settings().get(CLAIM_PUBLIC_KEY_SETTINGS_KEY)
                    {
                        new_settings.insert(CLAIM_PUBLIC_KEY_SETTINGS_KEY.to_string(), claim_key.clone());
                    }
                    let settings_changed = *instance.settings() != new_settings;
                    let envs_changed = instance.envs() != instance_config.envs;
                    if settings_changed || envs_changed {
                        info!(
                            "📝 Config changed for instance '{}' (settings_changed={}, envs_changed={}). Restart to \
                             apply.",
                            expected_name, settings_changed, envs_changed,
                        );
                        instance.set_settings(new_settings);
                        instance.set_envs(instance_config.envs.clone());
                        instance.set_config_dirty(true);
                    }
                }
            }
        }
        self.global_settings = new_global_settings;
        self.config = new_instance_configs.to_vec();
    }

    /// Marks every instance of `instance_type` as config dirty so that the user knows to restart them.
    pub fn mark_config_dirty(&mut self, instance_type: InstanceType) {
        self.instances_mut()
            .filter(|inst| inst.instance_type() == instance_type)
            .for_each(|inst| inst.set_config_dirty(true));
    }

    fn next_instance_id(&mut self) -> InstanceId {
        let id = self.instance_id;
        self.instance_id += 1;
        id
    }
}

fn forward_logs<R: AsyncRead + Unpin + Send + 'static>(path: PathBuf, reader: R, target: String) {
    let mut lines = BufReader::new(reader).lines();
    task::spawn(async move {
        let mut log_file = match File::create(path).await {
            Ok(file) => file,
            Err(err) => {
                log::error!("Failed to create log file for {target}: {err}");
                return;
            },
        };
        loop {
            let output = match lines.next_line().await {
                Ok(Some(output)) => output,
                Ok(None) => break,
                Err(err) => {
                    log::warn!("forward_logs read error ({target}): {err}");
                    break;
                },
            };
            log::debug!(target: FORWARDED_TARGET, "[{target}] {output}");
            if let Err(err) = log_file.write_all(output.as_bytes()).await {
                log::error!("forward_logs: {err}");
                return;
            }
            if let Err(err) = log_file.write_all(b"\n").await {
                log::error!("forward_logs: {err}");
                return;
            }
            if let Err(err) = log_file.flush().await {
                log::error!("forward_logs: {err}");
                return;
            }
        }
        log::debug!(target: FORWARDED_TARGET, "Process exited ({target})");
    });
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A stand-in base node binary that ignores its arguments and stays up long enough to pass the
    /// start-up check.
    async fn stub_executable(dir: &std::path::Path) -> Executable {
        let path = dir.join("stub_minotari_node");
        fs::write(&path, "#!/bin/sh\nexec sleep 30\n").await.unwrap();
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        Executable {
            instance_type: InstanceType::MinoTariNode,
            path,
        }
    }

    #[tokio::test]
    async fn create_instance_forks_a_new_name_with_the_configured_settings_and_rejects_a_taken_one() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path().to_path_buf();
        let executable = stub_executable(&base_path).await;

        let mut config = InstanceConfig::new(InstanceType::MinoTariNode)
            .with_setting("configured", "node")
            .with_setting("overridden", "node");
        config.envs = vec![("CONFIGURED_ENV".to_string(), "1".to_string())];
        let global_settings = HashMap::from([
            ("public_ip".to_string(), "10.0.0.1".to_string()),
            ("configured".to_string(), "global".to_string()),
        ]);
        let mut manager = InstanceManager::new(
            base_path.clone(),
            Network::LocalNet,
            global_settings,
            vec![config],
            47000,
        );

        let id = manager
            .create_instance(
                &executable,
                InstanceType::MinoTariNode,
                Some("extra-node".to_string()),
                HashMap::from([("overridden".to_string(), "request".to_string())]),
            )
            .await
            .unwrap();

        let instance = manager.instances().find(|i| i.id() == id).unwrap();
        assert_eq!(instance.name(), "extra-node");
        assert_eq!(
            *instance.settings(),
            HashMap::from([
                ("public_ip".to_string(), "10.0.0.1".to_string()),
                ("configured".to_string(), "node".to_string()),
                ("overridden".to_string(), "request".to_string()),
            ])
        );
        assert_eq!(instance.envs(), [("CONFIGURED_ENV".to_string(), "1".to_string())]);

        let err = manager
            .create_instance(
                &executable,
                InstanceType::MinoTariNode,
                Some("extra-node".to_string()),
                HashMap::new(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");

        let err = manager
            .create_instance(
                &executable,
                InstanceType::MinoTariNode,
                Some("Extra Node".to_string()),
                HashMap::new(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(manager.instances().count(), 1);
    }

    #[tokio::test]
    async fn an_unnamed_instance_takes_the_first_free_config_name() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path().to_path_buf();
        let executable = stub_executable(&base_path).await;
        let config = InstanceConfig::new(InstanceType::MinoTariNode).with_name("Node");
        let mut manager = InstanceManager::new(
            base_path.clone(),
            Network::LocalNet,
            HashMap::new(),
            vec![config],
            47200,
        );

        let mut create = async |name: Option<&str>| {
            let id = manager
                .create_instance(
                    &executable,
                    InstanceType::MinoTariNode,
                    name.map(ToString::to_string),
                    HashMap::new(),
                )
                .await
                .unwrap();
            manager.instances().find(|i| i.id() == id).unwrap().name().to_string()
        };

        assert_eq!(create(None).await, "Node-#00");
        assert_eq!(create(Some("Node-#01")).await, "Node-#01");
        assert_eq!(create(Some("unrelated")).await, "unrelated");
        assert_eq!(create(None).await, "Node-#02");
        assert!(base_path.join("processes").join(slugify("Node-#02")).is_dir());
    }

    #[tokio::test]
    async fn a_validator_without_a_claim_key_is_not_forked() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path().to_path_buf();
        let executable = Executable {
            instance_type: InstanceType::TariValidatorNode,
            ..stub_executable(&base_path).await
        };
        let mut manager = InstanceManager::new(base_path, Network::LocalNet, HashMap::new(), vec![], 47100);

        let err = manager
            .create_instance(
                &executable,
                InstanceType::TariValidatorNode,
                Some("vn".to_string()),
                HashMap::new(),
            )
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("no fee claim public key"), "{err}");
        assert!(err.contains(CLAIM_KEY_FILE_NAME), "{err}");
        assert_eq!(manager.instances().count(), 0);
    }

    #[tokio::test]
    async fn validator_claim_key_respects_explicit_override_and_config_reload() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path().to_path_buf();
        let node_exec = stub_executable(&base_path).await;
        let vn_exec = Executable {
            instance_type: InstanceType::TariValidatorNode,
            path: node_exec.path.clone(),
        };
        let processes_path = base_path.join("processes");
        fs::create_dir_all(&processes_path).await.unwrap();
        fs::write(
            processes_path.join(CLAIM_KEY_FILE_NAME),
            r#"{"account_public_key":"file-claim-key"}"#,
        )
        .await
        .unwrap();

        let vn_config = InstanceConfig::new(InstanceType::TariValidatorNode);
        let vn_default_name = vn_config.instance_name(0);
        let mut manager = InstanceManager::new(
            base_path,
            Network::LocalNet,
            HashMap::new(),
            vec![vn_config.clone()],
            47200,
        );
        manager
            .create_instance(&node_exec, InstanceType::MinoTariNode, "bn".to_string(), HashMap::new())
            .await
            .unwrap();

        let default_vn_id = manager
            .create_instance(
                &vn_exec,
                InstanceType::TariValidatorNode,
                vn_default_name,
                HashMap::new(),
            )
            .await
            .unwrap();
        let custom_settings = HashMap::from([(
            CLAIM_PUBLIC_KEY_SETTINGS_KEY.to_string(),
            "custom-claim-key".to_string(),
        )]);
        let override_vn_id = manager
            .create_instance(
                &vn_exec,
                InstanceType::TariValidatorNode,
                "vn-override".to_string(),
                custom_settings,
            )
            .await
            .unwrap();

        let default_vn = manager.instances().find(|i| i.id() == default_vn_id).unwrap();
        assert_eq!(
            default_vn.settings().get(CLAIM_PUBLIC_KEY_SETTINGS_KEY),
            Some(&"file-claim-key".to_string())
        );
        let override_vn = manager.instances().find(|i| i.id() == override_vn_id).unwrap();
        assert_eq!(
            override_vn.settings().get(CLAIM_PUBLIC_KEY_SETTINGS_KEY),
            Some(&"custom-claim-key".to_string())
        );

        manager.apply_config_update(HashMap::new(), &[vn_config]);
        let default_vn = manager.instances().find(|i| i.id() == default_vn_id).unwrap();
        assert!(!default_vn.is_config_dirty());
        assert_eq!(
            default_vn.settings().get(CLAIM_PUBLIC_KEY_SETTINGS_KEY),
            Some(&"file-claim-key".to_string())
        );
    }
}
