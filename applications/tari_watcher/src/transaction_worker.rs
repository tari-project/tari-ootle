// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::bail;
use log::*;
use tari_dan_common_types::layer_one_transaction::LayerOneTransactionDef;
use tokio::{
    fs,
    time::{self, Duration},
};

use crate::{
    config::Config,
    helpers::{read_registration_file, to_vn_public_keys},
    manager::ManagerHandle,
};

// TODO: make configurable
// Amount of time to wait before the watcher runs a check again
const REGISTRATION_LOOP_INTERVAL: Duration = Duration::from_secs(30);

// Periodically checks that the local node is still registered on the network.
// If it is no longer registered or close to expiry (1 epoch of blocks or less), it will attempt to re-register.
// It will do nothing if it is registered already and not close to expiry.
pub async fn worker_loop(config: Config, handle: ManagerHandle) -> anyhow::Result<ManagerHandle> {
    let mut is_registered = false;
    let vn_registration_file = config.get_registration_file();
    let vn_layer_one_transactions = config.get_layer_one_transaction_path();

    loop {
        time::sleep(REGISTRATION_LOOP_INTERVAL).await;

        if !is_registered {
            if config.auto_register {
                match ensure_registered(&handle, &vn_registration_file).await {
                    Ok(_) => {
                        is_registered = true;
                    },
                    Err(e) => {
                        error!("Unable to ensure validator registration: {}", e);
                    },
                }
            } else {
                debug!("Auto registration is disabled, skipping registration check");
            }
        }

        check_and_submit_layer_one_transactions(&handle, &vn_layer_one_transactions).await?;
    }
}

async fn ensure_registered(handle: &ManagerHandle, vn_registration_file: &PathBuf) -> anyhow::Result<()> {
    let Some(vn_reg_data) = read_registration_file(&vn_registration_file).await? else {
        info!("No registration data found, will try again in 30s");
        return Ok(());
    };
    let public_key = vn_reg_data.public_key;
    debug!("Local public key: {}", public_key.clone());

    let tip_info = handle.get_tip_info().await?;

    let current_height = tip_info.height();

    let vn_status = handle.get_active_validator_nodes().await?;
    let active_keys = to_vn_public_keys(vn_status);
    info!("Amount of active validator node keys: {}", active_keys.len());
    for key in &active_keys {
        info!("{}", key);
    }

    // if the node is already registered
    if active_keys.iter().any(|vn| *vn == public_key) {
        info!("VN has an active registration");
        return Ok(());
    }

    info!("VN not active, attempting to register..");
    let tx = handle.register_validator_node(current_height).await?;
    if !tx.is_success {
        bail!("Failed to register VN: {}", tx.failure_message);
    }
    info!(
        "Registered VN at block {} with transaction id: {}",
        current_height, tx.transaction_id
    );

    Ok(())
}

async fn check_and_submit_layer_one_transactions(
    handle: &ManagerHandle,
    vn_layer_one_transactions: &PathBuf,
) -> anyhow::Result<()> {
    let complete_dir = vn_layer_one_transactions.join("complete");
    fs::create_dir_all(&complete_dir).await?;
    let failed_dir = vn_layer_one_transactions.join("failed");
    fs::create_dir_all(&failed_dir).await?;

    info!("Checking for layer one transactions to submit..");
    let mut files = fs::read_dir(vn_layer_one_transactions).await?;
    while let Some(file) = files.next_entry().await.transpose() {
        let file = file?;
        if file.path() == complete_dir || file.path() == failed_dir {
            continue;
        }
        if !file.file_type().await?.is_file() {
            trace!("Skipping non-file: {}", file.path().display());
            continue;
        }
        if file.path().extension().map_or(true, |s| s != "json") {
            debug!("Skipping non-JSON file: {}", file.path().display());
            continue;
        }

        let f = match fs::File::open(file.path()).await {
            Ok(f) => f.into_std().await,
            Err(e) => {
                warn!("Failed to open file: {}", e);
                continue;
            },
        };
        match serde_json::from_reader::<_, LayerOneTransactionDef<serde_json::Value>>(f) {
            Ok(transaction_def) => {
                info!("Submitting {} transaction", transaction_def.proof_type);
                if let Err(err) = handle.submit_transaction(transaction_def).await {
                    warn!(
                        "Failed to submit transaction: {}. Moving to {}",
                        err,
                        failed_dir.display()
                    );
                    fs::rename(file.path(), failed_dir.join(file.file_name())).await?;
                    continue;
                }
                info!(
                    "Transaction submitted successfully! Moving to complete: {}",
                    complete_dir.display()
                );
                fs::rename(file.path(), complete_dir.join(file.file_name())).await?;
            },
            Err(e) => {
                warn!("Failed to parse JSON file {}: {}", file.path().display(), e);
                fs::rename(file.path(), failed_dir.join(file.file_name())).await?;
                continue;
            },
        }
    }

    Ok(())
}
