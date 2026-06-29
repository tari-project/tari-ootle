//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use anyhow::anyhow;
use minotari_app_grpc::tari_rpc::{
    BuildInfo,
    CreateTemplateRegistrationRequest,
    TemplateType,
    WasmInfo,
    template_type,
};
use tari_engine::wasm::WasmModule;
use tari_engine_types::hashing::hash_template_code;
use tari_ootle_walletd_client::{
    ComponentAddressOrName,
    types::{PublishTemplateRequest, TransactionWaitResultRequest},
};
use tari_template_lib_types::{TemplateAddress, constants::TARI_TOKEN};
use tari_template_test_tooling::compile::compile_template;

use crate::{
    TariWorld,
    wallet_daemon_client::{get_auth_wallet_daemon_client, get_balance},
};

#[derive(Debug, Clone)]
pub struct RegisteredTemplate {
    pub name: String,
    pub address: TemplateAddress,
}

pub async fn publish_template(
    world: &mut TariWorld,
    wallet_daemon_name: String,
    account_name: String,
    template_name: String,
) -> anyhow::Result<TemplateAddress> {
    // compile and load wasm
    let module = compile_wasm_template(template_name.clone())?;
    let wasm_binary = module.into_code().into_vec();

    // The publish fee scales with template size (storage + size premium), so it is not known up
    // front. Dry-run with the account's full balance as the cap to learn the required fee, then
    // submit with a little headroom. This keeps the test independent of the exact fee schedule.
    let balance = get_balance(world, &account_name, &wallet_daemon_name, TARI_TOKEN).await;
    let balance = u64::try_from(balance.to_u128()).unwrap_or(u64::MAX);

    let mut client = get_auth_wallet_daemon_client(world, &wallet_daemon_name).await;
    let dry_run = client
        .publish_template(PublishTemplateRequest {
            binary: wasm_binary.clone(),
            fee_account: Some(ComponentAddressOrName::Name(account_name.clone())),
            max_fee: balance,
            detect_inputs: true,
            dry_run: true,
            metadata: None,
        })
        .await?;
    let required_fee = dry_run
        .dry_run_fee
        .ok_or_else(|| anyhow!("publish_template dry run did not return a fee"))?;
    // ~10% headroom over the estimate, capped at the account balance.
    let max_fee = required_fee.saturating_add(required_fee / 10).min(balance);

    let response = client
        .publish_template(PublishTemplateRequest {
            binary: wasm_binary,
            fee_account: Some(ComponentAddressOrName::Name(account_name)),
            max_fee,
            detect_inputs: true,
            dry_run: false,
            metadata: None,
        })
        .await?;

    let tx_resp = client
        .wait_transaction_result(TransactionWaitResultRequest {
            transaction_id: response.transaction_id,
            timeout_secs: Some(60 * 5),
        })
        .await?;

    if tx_resp.timed_out {
        return Err(anyhow!(format!("Transaction timed out: {}", response.transaction_id)));
    }

    let finalize_result = tx_resp.result.ok_or(anyhow!(format!(
        "Missing transaction result: {}",
        response.transaction_id
    )))?;

    if let Some(reason) = finalize_result.result.any_reject() {
        return Err(anyhow!(format!(
            "Invalid transaction {}: Status: {}, Reason: {}",
            response.transaction_id, tx_resp.status, reason
        )));
    }

    // look for the new UP template substate
    let template_id = finalize_result
        .result
        .any_accept()
        .and_then(|result| result.up_iter().find_map(|(substate_id, _)| substate_id.as_template()))
        .map(|id| id.as_hash())
        .ok_or_else(|| anyhow!("Transaction result did not contain a published template!"))?;

    Ok(TemplateAddress::try_from_slice(template_id.as_slice())?)
}

pub async fn send_template_registration(
    world: &mut TariWorld,
    template_name: String,
    wallet_name: String,
) -> anyhow::Result<TemplateAddress> {
    let module = compile_wasm_template(template_name.clone())?;
    let binary_sha = hash_template_code(module.code());

    // publish the wasm file into http to be able to be fetched by the VN later
    let wasm_file_path = get_template_wasm_path(template_name.clone());

    let mock = world
        .get_mock_server()
        .publish_file(template_name.clone(), wasm_file_path.display().to_string())
        .await;

    // build the template registration request
    let request = CreateTemplateRegistrationRequest {
        template_name,
        template_version: 0,
        template_type: Some(TemplateType {
            template_type: Some(template_type::TemplateType::Wasm(WasmInfo { abi_version: 1 })),
        }),
        build_info: Some(BuildInfo {
            repo_url: "".to_string(),
            commit_hash: vec![],
        }),
        // repo_url: String::new(),
        // commit_hash: vec![],
        binary_sha: binary_sha.to_vec(),
        binary_url: mock.url,
        sidechain_deployment_key: vec![],
        fee_per_gram: 1,
    };

    // send the template registration request
    let wallet = world.get_wallet(&wallet_name);
    let mut client = wallet.create_client().await;

    // store the template address for future reference
    let resp = client.create_template_registration(request).await?.into_inner();
    Ok(TemplateAddress::try_from_slice(&resp.template_address).unwrap())
}

pub fn compile_wasm_template(template_name: String) -> Result<WasmModule, anyhow::Error> {
    let mut template_path = get_template_root_path();

    template_path.push(template_name);
    let wasm_module = compile_template(template_path.as_path(), &[])?;
    Ok(wasm_module)
}

pub fn get_template_wasm_path(template_name: String) -> PathBuf {
    let mut wasm_path = get_template_root_path();
    wasm_path.push(template_name.clone());
    wasm_path.push(format!("target/wasm32-unknown-unknown/release/{}.wasm", template_name));

    wasm_path
}

fn get_template_root_path() -> PathBuf {
    let mut template_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    template_path.push("src/templates");
    template_path
}
