//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use std::time::Duration;

use anyhow::anyhow;
use axum::headers::authorization::Bearer;
use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
use futures::{future, future::Either};
use log::*;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::ToByteType;
use tari_ootle_common_types::{optional::Optional, Epoch, Network};
use tari_ootle_wallet_sdk::{
    apis::{config::ConfigKey, key_manager::KeyBranch, transaction::TransactionApiError},
    network::WalletQueryErrorStatus,
};
use tari_transaction::{args, Transaction};
use tari_transaction_manifest::parse_manifest;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        CallInstructionRequest,
        PublishTemplateRequest,
        PublishTemplateResponse,
        TransactionGetAllRequest,
        TransactionGetAllResponse,
        TransactionGetRequest,
        TransactionGetResponse,
        TransactionGetResultRequest,
        TransactionGetResultResponse,
        TransactionSubmitDryRunRequest,
        TransactionSubmitDryRunResponse,
        TransactionSubmitManifestRequest,
        TransactionSubmitManifestResponse,
        TransactionSubmitRequest,
        TransactionSubmitResponse,
        TransactionWaitResultRequest,
        TransactionWaitResultResponse,
    },
};
use tokio::time;

use super::context::HandlerContext;
use crate::{
    handlers::{
        helpers::{get_account, get_account_or_default, invalid_params, not_found, transaction_rejected},
        wasm_optimizer::optimize_wasm_template,
        HandlerError,
    },
    services::{transaction_service::TransactionServiceError, WalletEvent},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::transaction";

pub async fn handle_submit_instruction(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: CallInstructionRequest,
) -> Result<TransactionSubmitResponse, anyhow::Error> {
    // TODO: fine-grained checks of individual addresses involved (resources, components, etc)
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let mut builder = context.transaction_builder().with_instructions(req.instructions);
    let sdk = context.wallet_sdk();

    if let Some(ref dump_account) = req.dump_outputs_into {
        let dump_account = get_account(dump_account, &sdk.accounts_api())?;
        builder = builder.put_last_instruction_output_on_workspace("bucket").call_method(
            *dump_account.component_address(),
            "deposit",
            args![Workspace("bucket")],
        );
    }
    let fee_account = get_account(&req.fee_account, &sdk.accounts_api())?;

    let transaction = builder
        .fee_transaction_pay_from_component(*fee_account.component_address(), req.max_fee)
        .with_min_epoch(req.min_epoch.map(Epoch))
        .with_max_epoch(req.max_epoch.map(Epoch))
        .build_unsigned_transaction();

    let request = TransactionSubmitRequest {
        transaction,
        signing_key_index: Some(fee_account.key_index()),
        detect_inputs: req.override_inputs.unwrap_or_default(),
        detect_inputs_use_unversioned: true,
        proof_ids: vec![],
    };
    handle_submit(context, token, request).await
}

pub async fn handle_submit(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionSubmitRequest,
) -> Result<TransactionSubmitResponse, anyhow::Error> {
    // TODO: fine-grained checks of individual addresses involved (resources, components, etc)
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk();
    let key_api = sdk.key_manager_api();
    // Fetch the key to sign the transaction
    // TODO: Ideally the SDK should take care of signing the transaction internally
    let (_, key) = key_api.get_key_or_active(KeyBranch::Account, req.signing_key_index)?;

    let detected_inputs = if req.detect_inputs {
        // If we are not overriding inputs, we will use inputs that we know about in the local substate id db
        let substates = req.transaction.to_referenced_substates()?;
        let substates = substates
            .into_iter()
            .chain(
                req.transaction
                    .inputs()
                    .into_iter()
                    .map(|req| req.substate_id().clone()),
            )
            .collect::<Vec<_>>();
        let loaded_substates = sdk
            .substate_api()
            .locate_dependent_substates(&substates, req.detect_inputs_use_unversioned)
            .await?;
        loaded_substates
            .into_iter()
            .map(|input| {
                if req.detect_inputs_use_unversioned {
                    input.into_unversioned()
                } else {
                    input
                }
            })
            .collect()
    } else {
        vec![]
    };

    info!(
        target: LOG_TARGET,
        "Detected {} input(s) (detect_inputs = {}, detect_inputs_use_unversioned = {})",
        detected_inputs.len(),
        req.detect_inputs,
        req.detect_inputs_use_unversioned,
    );

    let transaction = context
        .transaction_builder()
        .with_unsigned_transaction(req.transaction)
        .with_inputs(detected_inputs)
        .build_and_seal(&key.key);

    if log_enabled!(log::Level::Debug) {
        for input in transaction.inputs() {
            debug!(target: LOG_TARGET, "Input: {}", input)
        }
    }

    let tx_id = transaction.calculate_id();
    for proof_id in req.proof_ids {
        // update the proofs table with the corresponding transaction hash
        sdk.confidential_outputs_api()
            .locks_set_transaction_id(proof_id, tx_id)?;
    }

    info!(
        target: LOG_TARGET,
        "Submitted transaction with hash {}",
        transaction.calculate_id()
    );

    let transaction_id = context
        .transaction_service()
        .submit_transaction(transaction)
        .await
        .map_err(|e| {
            error!(target: LOG_TARGET, "Transaction submission failed: {}", e);
            match &e {
                TransactionServiceError::TransactionApiError(TransactionApiError::NetworkInterfaceError {
                    status,
                    message,
                }) => match &status {
                    WalletQueryErrorStatus::TransactionRejected { .. } => transaction_rejected(message),
                    WalletQueryErrorStatus::NotFound { message } => not_found(message),
                    _ => JsonRpcError::new(
                        JsonRpcErrorReason::ApplicationError(1),
                        format!("Failed to submit transaction: {}", e),
                        serde_json::Value::Null,
                    )
                    .into(),
                },
                _ => JsonRpcError::new(
                    JsonRpcErrorReason::ApplicationError(1),
                    format!("Failed to submit transaction: {}", e),
                    serde_json::Value::Null,
                )
                .into(),
            }
        })?;

    Ok(TransactionSubmitResponse { transaction_id })
}

pub async fn handle_submit_dry_run(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionSubmitDryRunRequest,
) -> Result<TransactionSubmitDryRunResponse, anyhow::Error> {
    // TODO: fine-grained checks of individual addresses involved (resources, components, etc)
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk();
    let key_api = sdk.key_manager_api();
    // Fetch the key to sign the transaction
    // TODO: Ideally the SDK should take care of signing the transaction internally
    let (_, key) = key_api.get_key_or_active(KeyBranch::Account, req.signing_key_index)?;

    let detected_inputs = if req.detect_inputs {
        // If we are not overriding inputs, we will use inputs that we know about in the local substate id db
        let substates = req.transaction.to_referenced_substates()?;
        let substates = substates.into_iter().collect::<Vec<_>>();
        let dependencies = sdk
            .substate_api()
            .locate_dependent_substates(&substates, req.detect_inputs_use_unversioned)
            .await?;
        dependencies
            .into_iter()
            .map(|input| {
                if req.detect_inputs_use_unversioned {
                    input.into_unversioned()
                } else {
                    input
                }
            })
            .collect()
    } else {
        vec![]
    };

    let transaction = context
        .transaction_builder()
        .with_unsigned_transaction(req.transaction)
        .with_inputs(detected_inputs)
        .with_dry_run(true)
        .build_and_seal(&key.key);

    for proof_id in req.proof_ids {
        // update the proofs table with the corresponding transaction hash
        sdk.confidential_outputs_api()
            .locks_set_transaction_id(proof_id, transaction.calculate_id())?;
    }

    info!(
        target: LOG_TARGET,
        "Submitted transaction with hash {}",
        transaction.calculate_id()
    );
    let exec_result = context
        .transaction_service()
        .submit_dry_run_transaction(transaction)
        .await?;

    Ok(TransactionSubmitDryRunResponse {
        transaction_id: exec_result.finalize.transaction_hash.into_array().into(),
        result: exec_result,
    })
}

pub async fn handle_submit_manifest(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionSubmitManifestRequest,
) -> Result<TransactionSubmitManifestResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk();

    let variables = req
        .variables
        .iter()
        .map(|(name, value)| {
            value.parse().map(|value| (name.to_string(), value)).map_err(|err| {
                invalid_params(
                    "variables",
                    Some(format!("Failed to parse variable '{}': {}", name, err)),
                )
            })
        })
        .collect::<Result<_, _>>()?;

    let instructions = parse_manifest(&req.manifest, variables, Default::default())
        .map_err(|e| invalid_params("manifest", Some(format!("Failed to parse manifest: {}", e))))?;

    let default_account = get_account_or_default(None, &sdk.accounts_api())?;

    let signing_key_index = req.signing_key_index.unwrap_or(default_account.key_index());
    let (_, key) = sdk
        .key_manager_api()
        .get_key_or_active(KeyBranch::Account, Some(signing_key_index))?;
    let seal_signer_pk = RistrettoPublicKey::from_secret_key(&key.key);

    let network = context.wallet_sdk().config_api().get::<Network>(ConfigKey::Network)?;

    let fee_amount = req.max_fee;

    let acc_key = sdk.key_manager_api().derive_account_key(default_account.key_index())?;
    let builder = Transaction::builder()
        .for_network(network.as_byte())
        .with_fee_instructions_builder(|builder| {
            if instructions.fee_instructions.is_empty() {
                builder.call_method(*default_account.component_address(), "pay_fee", args![fee_amount])
            } else {
                builder.with_instructions(instructions.fee_instructions)
            }
        })
        .with_instructions(instructions.instructions)
        .then(|builder| {
            if signing_key_index == default_account.key_index() {
                builder
            } else {
                builder.add_signer(&seal_signer_pk.to_byte_type(), &acc_key.key)
            }
        });
    let signatures = builder.signatures().to_vec();
    let mut transaction = builder.build_unsigned_transaction();

    // Detect inputs
    let substates = transaction.to_referenced_substates()?;
    let substates = substates.into_iter().collect::<Vec<_>>();
    let dependencies = sdk.substate_api().locate_dependent_substates(&substates, true).await?;
    let inputs = dependencies.into_iter().map(|input| input.into_unversioned());

    // set currently requested dry run status
    transaction.set_dry_run(req.dry_run);

    let transaction = transaction
        .with_inputs(inputs)
        .authorized_sealed_signer()
        .build(signatures)
        .seal(&key.key);

    if req.dry_run {
        let exec_result = context
            .transaction_service()
            .submit_dry_run_transaction(transaction)
            .await?;

        if let Some(reject) = exec_result.finalize.any_reject() {
            return Err(JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(5),
                format!("Dry-run transaction rejected: {reject}"),
                serde_json::Value::Null,
            )
            .into());
        }
        return Ok(TransactionSubmitManifestResponse {
            transaction_id: exec_result.finalize.transaction_hash.into_array().into(),
            result: Some(exec_result),
        });
    }

    let transaction_id = context.transaction_service().submit_transaction(transaction).await?;

    Ok(TransactionSubmitManifestResponse {
        transaction_id,
        result: None,
    })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionGetRequest,
) -> Result<TransactionGetResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionGet])?;
    let transaction = context
        .wallet_sdk()
        .transaction_api()
        .get(req.transaction_id)
        .optional()?
        .ok_or(HandlerError::NotFound)?;

    Ok(TransactionGetResponse {
        transaction: transaction.transaction,
        result: transaction.finalize,
        status: transaction.status,
        invalid_reason: transaction.invalid_reason,
        last_update_time: transaction.last_update_time,
    })
}

pub async fn handle_get_all(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionGetAllRequest,
) -> Result<TransactionGetAllResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionGet])?;
    let transactions =
        context
            .wallet_sdk()
            .transaction_api()
            .fetch_all(req.status, req.component, req.signer_public_key)?;
    Ok(TransactionGetAllResponse { transactions })
}

pub async fn handle_get_result(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionGetResultRequest,
) -> Result<TransactionGetResultResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionGet])?;
    let transaction = context
        .wallet_sdk()
        .transaction_api()
        .get(req.transaction_id)
        .optional()?
        .ok_or(HandlerError::NotFound)?;

    Ok(TransactionGetResultResponse {
        transaction_id: req.transaction_id,
        result: transaction.finalize,
        status: transaction.status,
    })
}

pub async fn handle_wait_result(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionWaitResultRequest,
) -> Result<TransactionWaitResultResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionGet])?;
    let mut events = context.notifier().subscribe();
    let transaction = context
        .wallet_sdk()
        .transaction_api()
        .get(req.transaction_id)
        .optional()?
        .ok_or(HandlerError::NotFound)?;

    if let Some(result) = transaction.finalize {
        return Ok(TransactionWaitResultResponse {
            transaction_id: req.transaction_id,
            result: Some(result),
            status: transaction.status,
            final_fee: transaction.final_fee.unwrap_or_default(),
            timed_out: false,
        });
    }

    let mut timeout = match req.timeout_secs {
        Some(timeout) => Either::Left(Box::pin(time::sleep(Duration::from_secs(timeout)))),
        None => Either::Right(future::pending()),
    };

    loop {
        let evt_or_timeout = tokio::select! {
            biased;
            event = events.recv() => {
                match event {
                    Ok(event) => Some(event),
                    Err(e) => return Err(anyhow!("Unexpected event stream error: {}", e)),
                }
            },
            _ = &mut timeout => None,
        };

        match evt_or_timeout {
            Some(WalletEvent::TransactionFinalized(event)) if event.transaction_id == req.transaction_id => {
                return Ok(TransactionWaitResultResponse {
                    transaction_id: req.transaction_id,
                    result: Some(event.finalize),
                    status: event.status,
                    final_fee: event.final_fee,
                    timed_out: false,
                });
            },
            Some(WalletEvent::TransactionInvalid(event)) if event.transaction_id == req.transaction_id => {
                return Ok(TransactionWaitResultResponse {
                    transaction_id: req.transaction_id,
                    result: event.finalize,
                    status: event.status,
                    final_fee: event.final_fee.unwrap_or_default(),
                    timed_out: false,
                });
            },
            Some(_) => continue,
            None => {
                return Ok(TransactionWaitResultResponse {
                    transaction_id: req.transaction_id,
                    result: None,
                    status: transaction.status,
                    final_fee: 0,
                    timed_out: true,
                });
            },
        };
    }
}

pub async fn handle_publish_template(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: PublishTemplateRequest,
) -> Result<PublishTemplateResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk();

    let fee_account = get_account_or_default(req.fee_account.as_ref(), &sdk.accounts_api())?;

    // trying to optimize WASM binary
    let wasm_binary = match optimize_wasm_template(req.binary.as_slice()).await {
        Ok(optimized) => {
            info!(target: LOG_TARGET, "WASM template optimized, original size: {} bytes, new size: {} bytes", req.binary.len(), optimized.len());
            optimized
        },
        Err(error) => {
            warn!(target: LOG_TARGET, "Error while optimizing WASM template (using original version now): {}", error);
            req.binary
        },
    };

    let transaction = context
        .transaction_builder()
        .fee_transaction_pay_from_component(*fee_account.component_address(), req.max_fee)
        .publish_template(wasm_binary)
        .build_unsigned_transaction();

    if req.dry_run {
        let request = TransactionSubmitDryRunRequest {
            transaction,
            signing_key_index: Some(fee_account.key_index()),
            detect_inputs: req.detect_inputs,
            detect_inputs_use_unversioned: true,
            proof_ids: vec![],
        };
        let resp = handle_submit_dry_run(context, token, request).await?;
        if let Some(reject) = resp.result.finalize.any_reject() {
            return Err(JsonRpcError::new(
                JsonRpcErrorReason::ApplicationError(5),
                format!("Dry-run transaction rejected: {reject}"),
                serde_json::Value::Null,
            )
            .into());
        }
        return Ok(PublishTemplateResponse {
            transaction_id: resp.transaction_id,
            dry_run_fee: Some(resp.result.finalize.fee_receipt.total_fees_charged()),
        });
    }
    let request = TransactionSubmitRequest {
        transaction,
        signing_key_index: Some(fee_account.key_index()),
        detect_inputs: req.detect_inputs,
        detect_inputs_use_unversioned: true,
        proof_ids: vec![],
    };
    let resp = handle_submit(context, token, request).await?;
    Ok(PublishTemplateResponse {
        transaction_id: resp.transaction_id,
        dry_run_fee: None,
    })
}
