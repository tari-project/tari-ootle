// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::*;
use minotari_app_grpc::tari_rpc::{GetActiveValidatorNodesResponse, RegisterValidatorNodeResponse};
use tari_dan_common_types::layer_one_transaction::LayerOneTransactionDef;
use tari_shutdown::ShutdownSignal;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    config::{Channels, Config},
    minotari::{MinotariNodes, TipStatus},
    monitoring::{process_status_alert, process_status_log, ProcessStatus, Transaction},
    process::{start_validator, ChildChannel},
};

pub struct ProcessManager {
    pub config: Config,
    pub shutdown_signal: ShutdownSignal, // listen for keyboard exit signal
    pub rx_request: mpsc::Receiver<ManagerRequest>,
    pub chain: MinotariNodes,
}

pub struct ChannelReceivers {
    pub rx_log: mpsc::Receiver<ProcessStatus>,
    pub rx_alert: mpsc::Receiver<ProcessStatus>,
    pub cfg_alert: Channels,
    pub task: JoinHandle<()>,
}

impl ProcessManager {
    pub fn new(config: Config, shutdown_signal: ShutdownSignal) -> (Self, ManagerHandle) {
        let (tx_request, rx_request) = mpsc::channel(1);
        let this = Self {
            shutdown_signal,
            rx_request,
            chain: MinotariNodes::new(
                config.base_node_grpc_url.clone(),
                config.base_wallet_grpc_url.clone(),
                config.get_registration_file(),
            ),
            config,
        };
        (this, ManagerHandle::new(tx_request))
    }

    pub async fn start_request_handler(mut self) -> anyhow::Result<ChannelReceivers> {
        info!("Starting validator node process");

        // clean_stale_pid_file(self.base_dir.clone().join(DEFAULT_VALIDATOR_PID_PATH)).await?;

        let cc = self.start_child_process().await;

        info!("Setup completed: connected to base node and wallet, ready to receive requests");
        let task_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(req) = self.rx_request.recv() => {
                        if let Err(err) = self.handle_request(req, &cc.tx_log, &cc.tx_alert).await {
                            error!("Error handling request: {}", err);
                        }
                    }

                    _ = self.shutdown_signal.wait() => {
                        info!("Shutting down process manager");
                        break;
                    }
                }
            }
        });

        Ok(ChannelReceivers {
            rx_log: cc.rx_log,
            rx_alert: cc.rx_alert,
            cfg_alert: cc.cfg_alert,
            task: task_handle,
        })
    }

    async fn handle_request(
        &mut self,
        req: ManagerRequest,
        tx_log: &mpsc::Sender<ProcessStatus>,
        tx_alert: &mpsc::Sender<ProcessStatus>,
    ) -> anyhow::Result<()> {
        match req {
            ManagerRequest::GetTipInfo { reply } => {
                let response = self.chain.get_tip_status().await?;
                drop(reply.send(Ok(response)));
            },
            ManagerRequest::GetActiveValidatorNodes { reply } => {
                let response = self.chain.get_active_validator_nodes().await;
                drop(reply.send(response));
            },
            ManagerRequest::RegisterValidatorNode { block, reply } => {
                let response = self.chain.register_validator_node().await;

                if let Ok(ref response) = response {
                    // send registration response to logger
                    if let Err(e) = tx_log
                        .send(ProcessStatus::Submitted(Transaction::new(
                            response.transaction_id,
                            block,
                        )))
                        .await
                    {
                        error!("Failed to send node registration update to monitoring: {}", e);
                    }
                    // send registration response to alerting
                    if let Err(e) = tx_alert
                        .send(ProcessStatus::Submitted(Transaction::new(
                            response.transaction_id,
                            block,
                        )))
                        .await
                    {
                        error!("Failed to send node registration update to alerting: {}", e);
                    }
                }

                drop(reply.send(response));
            },
            ManagerRequest::SubmitTransaction { transaction_def, reply } => {
                let response = self.chain.submit_transaction(transaction_def).await;
                let _ignore = reply.send(response);
            },
        }

        Ok(())
    }

    async fn start_child_process(&self) -> ChildChannel {
        let vn_binary_path = self.config.validator_node_executable_path.clone();
        let vn_base_dir = self.config.base_dir.join(self.config.vn_base_dir.clone());

        // get child channel to communicate with the validator node process
        let cc = start_validator(
            vn_binary_path,
            vn_base_dir,
            // TODO: just pass in config
            self.config.base_node_grpc_url.clone(),
            self.config.channel_config.clone(),
            self.config.auto_restart,
            self.config.network,
        )
        .await;
        if cc.is_none() {
            todo!("Create new validator node process event listener for fetched existing PID from OS");
        }

        cc.unwrap()
    }
}

pub async fn start_receivers(
    rx_log: mpsc::Receiver<ProcessStatus>,
    rx_alert: mpsc::Receiver<ProcessStatus>,
    cfg_alert: Channels,
) {
    // spawn logging and alerting tasks to process status updates
    tokio::spawn(async move {
        process_status_log(rx_log).await;
        warn!("Logging task has exited");
    });
    tokio::spawn(async move {
        process_status_alert(rx_alert, cfg_alert).await;
        warn!("Alerting task has exited");
    });
}

type Reply<T> = oneshot::Sender<anyhow::Result<T>>;

pub enum ManagerRequest {
    GetTipInfo {
        reply: Reply<TipStatus>,
    },
    GetActiveValidatorNodes {
        reply: Reply<Vec<GetActiveValidatorNodesResponse>>,
    },
    RegisterValidatorNode {
        block: u64,
        reply: Reply<RegisterValidatorNodeResponse>,
    },
    SubmitTransaction {
        transaction_def: LayerOneTransactionDef<serde_json::Value>,
        reply: Reply<()>,
    },
}

pub struct ManagerHandle {
    tx_request: mpsc::Sender<ManagerRequest>,
}

impl ManagerHandle {
    pub fn new(tx_request: mpsc::Sender<ManagerRequest>) -> Self {
        Self { tx_request }
    }

    pub async fn get_active_validator_nodes(&self) -> anyhow::Result<Vec<GetActiveValidatorNodesResponse>> {
        let (tx, rx) = oneshot::channel();
        self.tx_request
            .send(ManagerRequest::GetActiveValidatorNodes { reply: tx })
            .await?;
        rx.await?
    }

    pub async fn register_validator_node(&self, block: u64) -> anyhow::Result<RegisterValidatorNodeResponse> {
        let (tx, rx) = oneshot::channel();
        self.tx_request
            .send(ManagerRequest::RegisterValidatorNode { block, reply: tx })
            .await?;
        rx.await?
    }

    pub async fn submit_transaction(
        &self,
        transaction_def: LayerOneTransactionDef<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx_request
            .send(ManagerRequest::SubmitTransaction {
                transaction_def,
                reply: tx,
            })
            .await?;
        rx.await?
    }

    pub async fn get_tip_info(&self) -> anyhow::Result<TipStatus> {
        let (tx, rx) = oneshot::channel();
        self.tx_request.send(ManagerRequest::GetTipInfo { reply: tx }).await?;
        rx.await?
    }
}
