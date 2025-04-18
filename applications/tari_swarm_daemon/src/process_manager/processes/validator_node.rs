//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::Display,
    ops::ControlFlow,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::bail;
use log::info;
use tari_validator_node_client::{
    types::{LayerOneTransactionParams, PrepareLayerOneTransactionRequest},
    ValidatorNodeClient,
};
use tokio::time::sleep;
use url::Url;

use crate::process_manager::Instance;

pub struct ValidatorNodeProcess {
    instance: Instance,
}

impl ValidatorNodeProcess {
    pub fn new(instance: Instance) -> Self {
        Self { instance }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut Instance {
        &mut self.instance
    }

    pub fn connect_client(&self) -> anyhow::Result<ValidatorNodeClient> {
        let client = ValidatorNodeClient::connect(self.json_rpc_address())?;
        Ok(client)
    }

    pub async fn wait_for_startup(&self, timeout: Duration) -> anyhow::Result<()> {
        let client = self.connect_client()?;

        attempt(timeout, || {
            let mut client = client.clone();
            async move {
                match client.get_identity().await {
                    Ok(_) => Ok(ControlFlow::Break(())),
                    Err(err) => {
                        log::error!("Failed to connect to validator node: {}", err);
                        log::info!(
                            "Waiting for validator node {} ({}) to start up...",
                            self.instance().id(),
                            self.instance().name()
                        );
                        Ok(ControlFlow::Continue(()))
                    },
                }
            }
        })
        .await
    }

    pub async fn wait_for_initial_scanning_to_complete(&self, timeout: Duration) -> anyhow::Result<()> {
        let client = self.connect_client()?;

        attempt(timeout, || {
            let mut client = client.clone();
            async move {
                let stats = client.get_epoch_manager_stats().await?;
                if stats.is_initial_scanning_complete {
                    return Ok(ControlFlow::Break(()));
                }
                log::info!(
                    "Waiting for validator node {} ({}) to complete initial scanning...",
                    self.instance().id(),
                    self.instance().name()
                );
                Ok(ControlFlow::Continue(()))
            }
        })
        .await
    }

    pub async fn prepare_registration_transaction(&self) -> anyhow::Result<()> {
        let mut client = self.connect_client()?;
        client
            .prepare_layer_one_transaction(PrepareLayerOneTransactionRequest {
                params: LayerOneTransactionParams::Registration,
            })
            .await?;
        info!("🟢 Submitted validator node registration prepare request to {self}");
        Ok(())
    }

    pub async fn prepare_exit_transaction(&self) -> anyhow::Result<()> {
        let mut client = self.connect_client()?;
        client
            .prepare_layer_one_transaction(PrepareLayerOneTransactionRequest {
                params: LayerOneTransactionParams::Exit,
            })
            .await?;
        info!("🟢 Submitted validator node exit prepare request to {self}");
        Ok(())
    }

    pub fn json_rpc_address(&self) -> Url {
        let jrpc_port = self.instance().allocated_ports().get("jrpc").unwrap();
        Url::parse(&format!("http://localhost:{jrpc_port}/json_rpc")).unwrap()
    }

    pub fn layer_one_transaction_path(&self) -> PathBuf {
        self.instance.base_path().join("data/layer_one_transactions")
    }
}

impl Display for ValidatorNodeProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.instance().name(), self.instance().id())
    }
}

pub async fn attempt<F, Fut, T>(timeout: Duration, mut f: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<ControlFlow<T>>>,
{
    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            bail!("Operation timed out");
        }

        match f().await? {
            ControlFlow::Break(result) => return Ok(result),
            ControlFlow::Continue(_) => sleep(Duration::from_secs(1)).await,
        }
    }
}
