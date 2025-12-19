//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    path::PathBuf,
    pin::pin,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context};
use futures::future::Either;
use log::{debug, info};
use minotari_wallet_grpc_client::grpc;
use tari_ootle_app_utilities::configuration::convert_network_to_l1_network;
use tari_ootle_common_types::Network;
use tari_shutdown::ShutdownSignal;
use tari_transaction_components::consensus::NetworkConsensus;
use tari_validator_node_client::types::AddPeerRequest;
use tokio::{sync::mpsc, time, time::sleep};

use crate::{
    config::{Config, InstanceType},
    layer_one_transactions::LayerOneTransactionService,
    process_manager::{
        executables::ExecutableManager,
        handle::{ProcessManagerHandle, ProcessManagerRequest},
        instances::InstanceManager,
        InstanceId,
        MinoTariWalletProcess,
        MinotariNodeDetails,
    },
};

pub struct ProcessManager {
    executable_manager: ExecutableManager,
    instance_manager: InstanceManager,
    rx_request: mpsc::Receiver<ProcessManagerRequest>,
    shutdown_signal: ShutdownSignal,
    skip_registration: bool,
    enable_manual_connect: bool,
    network: Network,
}

impl ProcessManager {
    pub fn new(config: &Config, shutdown_signal: ShutdownSignal) -> (Self, ProcessManagerHandle) {
        let (tx_request, rx_request) = mpsc::channel(1);

        let mut global_settings = config.settings.clone();
        if let Some(public_ip) = config.public_ip {
            global_settings.insert("public_ip".to_string(), public_ip.to_string());
        }
        let this = Self {
            skip_registration: config.skip_registration,
            executable_manager: ExecutableManager::new(
                config.processes.executables.clone(),
                config.processes.force_compile,
            ),
            instance_manager: InstanceManager::new(
                config.base_dir.clone(),
                config.network,
                global_settings,
                config.processes.instances.clone(),
                config.start_port,
            ),
            rx_request,
            shutdown_signal,
            enable_manual_connect: config.enable_manual_validator_connect,
            network: config.network,
        };
        (this, ProcessManagerHandle::new(tx_request))
    }

    async fn start_up(&mut self) -> anyhow::Result<()> {
        info!("Starting process manager");
        let executables = self.executable_manager.prepare_all().await?;
        self.instance_manager.fork_all(executables).await?;

        // Wait some time for all instances to start
        sleep(Duration::from_secs(self.instance_manager.num_instances() as u64)).await;
        self.check_instances_running()?;

        Ok(())
    }

    async fn connect_all_validators(&mut self) -> anyhow::Result<()> {
        info!("Connecting all nodes");
        let mut clients = vec![];
        let mut ids = vec![];
        for vn in self.instance_manager.validator_nodes_mut() {
            match vn.connect_client() {
                Ok(mut client) => {
                    info!("🟢 Validator node {} connected", vn.instance().name());
                    let id = client.get_identity().await?;
                    clients.push(client);
                    ids.push(id);
                },
                Err(err) => {
                    log::error!("Failed to connect to validator node {}: {}", vn.instance().name(), err);
                },
            }
        }

        for (i, client) in clients.iter_mut().enumerate() {
            for (j, id) in ids.iter().enumerate() {
                if i == j {
                    continue; // Skip self
                }
                client
                    .add_peer(AddPeerRequest {
                        public_key: id.public_key,
                        addresses: id.public_addresses.clone(),
                        wait_for_dial: false,
                    })
                    .await?;
            }
        }
        Ok(())
    }

    async fn register_nodes_and_templates_as_required(
        &mut self,
        layer_one_transaction_service: Option<&mut LayerOneTransactionService>,
    ) -> anyhow::Result<()> {
        if self.skip_registration {
            return Ok(());
        }

        let layer_one_transaction_service = match layer_one_transaction_service {
            Some(service) => service,
            None => {
                return Err(anyhow!(
                    "No MinotariConsoleWallet available. Please start a wallet before registering validator nodes and \
                     templates or use --skip-registration flag to skip this.",
                ))
            },
        };

        // Add the watchers
        for vn in self.instance_manager.validator_nodes() {
            let l1_tx_path = vn.layer_one_transaction_path();
            tokio::fs::create_dir_all(&l1_tx_path).await?;
            layer_one_transaction_service.add_watch(l1_tx_path);
        }

        let num_blocks = self.instance_manager.num_validator_nodes();

        if num_blocks > 0 {
            // Mine some initial funds, guessing 10 blocks extra is sufficient for coinbase maturity
            self.mine(num_blocks + 10).await.context("initial mining failed")?;
            self.wait_for_wallet_funds(num_blocks as u64)
                .await
                .context("waiting for wallet funds")?;

            self.register_all_validator_nodes()
                .await
                .context("registering validator node via GRPC")?;

            // We need to process these now so that we can automatically mine once the transactions are submitted
            let num_validator_nodes = self.instance_manager.num_validator_nodes();
            let mut transaction_ids = vec![];
            loop {
                let transactions = layer_one_transaction_service.process_any().await?;
                transaction_ids.extend(
                    transactions
                        .iter()
                        .filter(|(transaction, _)| transaction.payload_type.is_validator_registration())
                        .map(|(_, tx_id)| *tx_id),
                );

                info!(
                    "🟢 {}/{} validator node registration submitted",
                    transaction_ids.len(),
                    num_validator_nodes
                );
                if transaction_ids.len() == num_validator_nodes {
                    break;
                }
                // 1
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            info!("🟢 All validator nodes registrations have been submitted to the wallet");

            // Wait to ensure the wallet has broadcast all transactions before mining
            self.wait_for_wallet_to_broadcast_transactions(transaction_ids).await?;

            // "Mine in" the validators and templates
            // 10 for new epoch + 10 for BL scan lag
            self.mine(20).await?;
        }

        Ok(())
    }

    fn check_instances_running(&mut self) -> anyhow::Result<()> {
        for instance in self.instance_manager.instances_mut().filter(|i| {
            !i.instance_type().is_tari_node() &&
                !i.instance_type().is_miner() &&
                !i.instance_type().is_wallet_daemon_create_key()
        }) {
            if let Some(status) = instance.check_running()? {
                return Err(anyhow!(
                    "Failed to start instance: {} {} {}",
                    instance.name(),
                    instance.instance_type(),
                    status
                ));
            }
        }
        Ok(())
    }

    fn clear_terminated_instances(&mut self) -> anyhow::Result<()> {
        let mut instances_to_remove = vec![];
        for instance in self.instance_manager.instances_mut() {
            if !instance.is_running() {
                // Already been checked
                continue;
            }
            if let Some(status) = instance.check_running()? {
                if status.success() {
                    info!(
                        "Instance exited cleanly: {} {}",
                        instance.name(),
                        instance.instance_type()
                    );
                } else {
                    log::error!(
                        "Instance exited with status {}: {} {}",
                        status.code().unwrap_or(-1),
                        instance.name(),
                        instance.instance_type()
                    );
                }
                // We only want to clear the miners, the rest we keep around to display that they are terminated
                if instance.instance_type().is_miner() {
                    instances_to_remove.push(instance.id());
                }
            }
        }

        for instance_id in instances_to_remove {
            self.instance_manager.remove_instance(instance_id)?;
        }
        Ok(())
    }

    pub async fn start(mut self) -> anyhow::Result<()> {
        let shutdown_signal = self.shutdown_signal.clone();

        {
            let fut = pin!(self.start_up());
            match shutdown_signal.clone().select(fut).await {
                Either::Left(_) => {
                    info!("Shutting down process manager");
                    return Ok(());
                },
                Either::Right((result, _)) => {
                    result?;
                },
            }
        }

        if self.enable_manual_connect {
            {
                info!("Connecting all validator nodes manually");
                let fut = pin!(self.connect_all_validators());
                match shutdown_signal.clone().select(fut).await {
                    Either::Left(_) => {
                        info!("Shutting down process manager during validator connection");
                        return Ok(());
                    },
                    Either::Right((result, _)) => {
                        result?;
                    },
                }
            }
        }

        let mut layer_one_transaction_service = self.create_layer_one_transaction_service().await?;

        {
            let fut = pin!(self.register_nodes_and_templates_as_required(layer_one_transaction_service.as_mut()));
            match shutdown_signal.clone().select(fut).await {
                Either::Left(_) => {
                    info!("Shutting down process manager");
                    return Ok(());
                },
                Either::Right((result, _)) => {
                    result?;
                },
            }
        }

        let mut interval = time::interval_at(
            time::Instant::from_std(Instant::now() + Duration::from_secs(5)),
            Duration::from_secs(5),
        );

        loop {
            tokio::select! {
                Some(req) = self.rx_request.recv() => {
                    if let Err(err) = self.handle_request(req, layer_one_transaction_service.as_mut()).await {
                        log::error!("Error handling request: {:?}", err);
                    }
                },

                _ = interval.tick() => {
                    if let Err(err) = self.on_tick(layer_one_transaction_service.as_mut()).await {
                        log::error!("(on_tick) Error: {:?}", err);
                    }
                },

                _ = self.shutdown_signal.wait() => {
                    info!("Shutting down process manager");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn on_tick(
        &mut self,
        layer_one_transaction_service: Option<&mut LayerOneTransactionService>,
    ) -> anyhow::Result<()> {
        self.clear_terminated_instances()?;

        let Some(layer_one_transaction_service) = layer_one_transaction_service else {
            debug!("🫙 No MinotariConsoleWallet available. Skipping layer one transaction processing");
            return Ok(());
        };

        // Check for layer one transactions
        let processed = layer_one_transaction_service.process_any().await?;
        if processed.is_empty() {
            debug!("🫙 No layer one transactions to process");
            return Ok(());
        }
        for (transaction, tx_id) in processed {
            info!(
                "🟢 {} transaction submitted by watcher (id: {})",
                transaction.payload_type, tx_id
            );
        }

        Ok(())
    }

    async fn create_layer_one_transaction_service(&self) -> anyhow::Result<Option<LayerOneTransactionService>> {
        let wallet = self.instance_manager.minotari_wallets().next();
        let Some(wallet) = wallet else {
            return Ok(None);
        };

        let client = wallet.connect_client().await?;
        let service = LayerOneTransactionService::init(client)?;
        Ok(Some(service))
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_request(
        &mut self,
        req: ProcessManagerRequest,
        layer_one_transaction_service: Option<&mut LayerOneTransactionService>,
    ) -> anyhow::Result<()> {
        use ProcessManagerRequest::*;
        match req {
            CreateInstance {
                name,
                instance_type,
                settings,
                reply,
            } => {
                let Some(instance) = self.instance_manager.instances().find(|i| i.name() == name) else {
                    if reply
                        .send(Err(anyhow!(
                            "Instance with name '{name}' already exists. Please choose a different name",
                        )))
                        .is_err()
                    {
                        log::warn!("Request cancelled before response could be sent")
                    }
                    return Ok(());
                };

                let envs = instance.envs().to_vec();

                let executable = self.executable_manager.get_executable(instance_type).ok_or_else(|| {
                    anyhow!(
                        "No configuration for instance type '{instance_type}'. Please add this to the configuration",
                    )
                })?;
                let result = self
                    .instance_manager
                    .fork_new(executable, instance_type, name, None, envs, settings)
                    .await;

                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            ListInstances { by_type, reply } => {
                let instances = self
                    .instance_manager
                    .instances()
                    .filter(|i| by_type.is_none() || i.instance_type() == by_type.unwrap())
                    .map(Into::into)
                    .collect();

                if reply.send(Ok(instances)).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            StartInstance { instance_id, reply } => {
                let executable = {
                    let instance = self
                        .instance_manager
                        .instances()
                        .find(|i| i.id() == instance_id)
                        .ok_or_else(|| anyhow!("Instance with ID '{}' not found", instance_id))?;
                    let instance_type = instance.instance_type();
                    self.executable_manager
                        .compile_executable_if_required(instance_type)
                        .await?
                };

                let result = self.instance_manager.start_instance(instance_id, executable).await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            StopInstance { instance_id, reply } => {
                let result = self.instance_manager.stop_instance(instance_id).await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            DeleteInstanceData { instance_id, reply } => {
                let result = self.instance_manager.delete_instance_data(instance_id).await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            MineBlocks { blocks, reply } => {
                let result = self.mine(blocks as usize).await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },

            RegisterValidatorNode { instance_id, reply } => {
                let Some(layer_one_transaction_service) = layer_one_transaction_service else {
                    if reply
                        .send(Err(anyhow!(
                            "No MinotariConsoleWallet available. Please start a wallet before registering validator \
                             nodes",
                        )))
                        .is_err()
                    {
                        log::warn!("Request cancelled before response could be sent")
                    }
                    return Ok(());
                };
                let result = self
                    .register_validator_node(instance_id, layer_one_transaction_service)
                    .await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            ExitValidatorNode { instance_id, reply } => {
                let result = self.exit_validator_node(instance_id).await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            BurnFunds {
                amount,
                wallet_instance_id,
                account_name,
                out_path,
                reply,
            } => {
                let result = self
                    .burn_funds_to_wallet_account(amount, wallet_instance_id, account_name, out_path)
                    .await;
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
            GetMinotariNodeDetails { instance_id, reply } => {
                let node = self
                    .instance_manager
                    .minotari_nodes()
                    .find(|i| i.instance().id() == instance_id)
                    .ok_or_else(|| anyhow!("MinotariNode with ID '{}' not found", instance_id))?;
                let result = node.get_chain_metadata().await.map(|metadata| MinotariNodeDetails {
                    instance_info: node.instance().into(),
                    height: metadata.map(|m| m.best_block_height),
                });
                if reply.send(result).is_err() {
                    log::warn!("Request cancelled before response could be sent")
                }
            },
        }

        Ok(())
    }

    async fn burn_funds_to_wallet_account(
        &mut self,
        amount: u64,
        wallet_instance_id: InstanceId,
        account_name: String,
        out_path: PathBuf,
    ) -> anyhow::Result<PathBuf> {
        let wallet = self
            .instance_manager
            .get_wallet_daemon_mut(wallet_instance_id)
            .ok_or_else(|| {
                anyhow!(
                    "No wallet daemon instances {wallet_instance_id} found. Please start a wallet before burning funds"
                )
            })?;
        let (claim_public_key, nonce_key_index) = wallet.create_nonce_key().await?;
        let wallet = self
            .instance_manager
            .minotari_wallets()
            .next()
            .ok_or_else(|| anyhow!("No MinoTariConsoleWallet instances found"))?;

        let burn_resp = wallet.burn_funds(amount, claim_public_key).await?;

        let file_name = PathBuf::from(format!("burn_proof-{}.json", burn_resp.tx_id));

        let client = wallet.connect_client().await?;
        let path = out_path.join(&file_name);
        tokio::spawn(MinoTariWalletProcess::wait_for_claim_burn_proof_task(
            client,
            path,
            burn_resp.commitment,
            amount,
            nonce_key_index,
        ));

        info!("🔥 Burned {amount} Tari to account {account_name}");
        Ok(file_name)
    }

    async fn register_all_validator_nodes(&mut self) -> anyhow::Result<()> {
        let mut skip = vec![];
        for vn in self.instance_manager.validator_nodes_mut() {
            if let Some(status) = vn.instance_mut().check_running()? {
                log::error!(
                    "Skipping registration for validator node {}: {} since it is not running: {}",
                    vn.instance().id(),
                    vn.instance().name(),
                    status
                );
                skip.push(vn.instance().id());
            }
        }

        for vn in self.instance_manager.validator_nodes() {
            if skip.contains(&vn.instance().id()) {
                continue;
            }
            info!("🟡 Registering validator node {}", vn.instance().name());
            if let Err(err) = vn.wait_for_startup(Duration::from_secs(10)).await {
                log::error!(
                    "Skipping registration for validator node {}: {} since it is not responding",
                    vn.instance().id(),
                    err
                );
                continue;
            }

            vn.wait_for_initial_scanning_to_complete(Duration::from_secs(10))
                .await
                .context("waiting for initial scanning to complete")?;

            vn.prepare_registration_transaction().await?;
        }
        Ok(())
    }

    async fn register_validator_node(
        &mut self,
        instance_id: InstanceId,
        layer_one_transaction_service: &mut LayerOneTransactionService,
    ) -> anyhow::Result<()> {
        let vn = self
            .instance_manager
            .validator_nodes()
            .find(|vn| vn.instance().id() == instance_id)
            .ok_or_else(|| anyhow!("Validator node with ID '{}' not found", instance_id))?;

        if !vn.instance().is_running() {
            log::error!(
                "Skipping registration for validator node {}: {} since it is not running",
                vn.instance().id(),
                vn.instance().name()
            );
            return Ok(());
        }

        if let Err(err) = vn.wait_for_startup(Duration::from_secs(10)).await {
            log::error!(
                "Skipping registration for validator node {}: {} since it is not responding",
                vn.instance().id(),
                err
            );
            return Ok(());
        }

        let l1_tx_path = vn.layer_one_transaction_path();
        tokio::fs::create_dir_all(&l1_tx_path).await?;
        // Watch for layer one transactions for this validator node
        layer_one_transaction_service.add_watch(l1_tx_path);

        vn.wait_for_initial_scanning_to_complete(Duration::from_secs(10))
            .await
            .context("waiting for initial scanning to complete")?;

        vn.prepare_registration_transaction().await?;

        Ok(())
    }

    async fn exit_validator_node(&mut self, instance_id: InstanceId) -> anyhow::Result<()> {
        let vn = self
            .instance_manager
            .validator_nodes()
            .find(|vn| vn.instance().id() == instance_id)
            .ok_or_else(|| anyhow!("Validator node with ID '{}' not found", instance_id))?;

        if !vn.instance().is_running() {
            log::error!(
                "Skipping exit for validator node {}: {} since it is not running",
                vn.instance().id(),
                vn.instance().name()
            );
            return Ok(());
        }

        // This VN is already watched, so the watcher will submit this
        vn.prepare_exit_transaction().await?;

        Ok(())
    }

    async fn mine(&mut self, blocks: usize) -> anyhow::Result<()> {
        if blocks == 0 {
            return Ok(());
        }
        let executable = self
            .executable_manager
            .get_executable(InstanceType::MinoTariMiner)
            .ok_or_else(|| {
                anyhow!("No executable configuration for 'MinoTariMiner'. Please add this to the configuration")
            })?;

        let settings = HashMap::from([("max_blocks".to_string(), blocks.to_string())]);
        let id = self
            .instance_manager
            .fork_new(
                executable,
                InstanceType::MinoTariMiner,
                "miner".to_string(),
                None,
                vec![],
                settings,
            )
            .await?;

        let status = self.instance_manager.wait(id).await?;
        if !status.success() {
            return Err(anyhow!("Failed to mine blocks. Process exited with {status}"));
        }

        Ok(())
    }

    async fn wait_for_wallet_funds(&mut self, min_expected_blocks: u64) -> anyhow::Result<()> {
        if min_expected_blocks == 0 {
            return Ok(());
        }
        // WARN: Assumes one wallet
        let wallet = self.instance_manager.minotari_wallets().next().ok_or_else(|| {
            anyhow!("No MinoTariConsoleWallet instances found. Please start a wallet before waiting for funds")
        })?;

        let constants = NetworkConsensus::from(convert_network_to_l1_network(&self.network))
            .create_consensus_constants()
            .pop()
            .unwrap();
        let initial_emission_amount = constants.emission_amounts().0;

        loop {
            let resp = wallet.get_balance().await?;
            if resp.available_balance > min_expected_blocks * initial_emission_amount.as_u64() {
                info!("💰 Wallet has funds. Available balance: {}", resp.available_balance);
                break;
            }
            sleep(Duration::from_secs(2)).await;
            info!(
                "💱 Waiting for wallet to mine some funds ({} uT / {})",
                resp.available_balance,
                min_expected_blocks * initial_emission_amount
            );
        }

        Ok(())
    }

    async fn wait_for_wallet_to_broadcast_transactions(&mut self, tx_ids: Vec<u64>) -> anyhow::Result<()> {
        // WARN: Assumes one wallet
        let wallet = self.instance_manager.minotari_wallets().next().ok_or_else(|| {
            anyhow!("No MinoTariConsoleWallet instances found. Please start a wallet before waiting for funds")
        })?;

        loop {
            let resp = wallet.get_transaction_info(tx_ids.clone()).await?;
            if resp.iter().all(|tx| {
                let status = tx.status();
                debug!("Transaction {} is {:?}", tx.tx_id, status);
                use grpc::TransactionStatus::*;
                !matches!(status, NotFound | Pending | Queued)
            }) {
                info!("📡 Wallet has broadcast all transactions");
                break;
            }
            sleep(Duration::from_secs(2)).await;
            info!("💱 Waiting for wallet broadcast {} transactions", tx_ids.len());
            wallet.revalidate_all_transactions().await?;
        }

        Ok(())
    }
}
