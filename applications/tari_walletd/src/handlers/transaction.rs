//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use std::time::Duration;

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
use futures::{future, future::Either};
use log::*;
use ootle_byte_type::ToByteType;
use tari_ootle_common_types::{Epoch, optional::Optional, response_status::ResponseErrorStatus};
use tari_ootle_transaction::args;
use tari_ootle_wallet_sdk::{apis::transaction::TransactionApiError, models::WalletEvent};
use tari_ootle_wallet_sdk_services::transaction_service::TransactionServiceError;
use tari_ootle_walletd_client::{
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
use tari_transaction_manifest::parse_manifest;
use tokio::time;

use super::context::HandlerContext;
use crate::{
    handlers::{
        HandlerError,
        helpers::{
            get_account,
            get_account_or_default,
            invalid_params,
            invalid_request,
            not_found,
            transaction_rejected,
        },
    },
    services::wasm_optimizer::optimize_wasm_template,
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
    let owner_key_id = fee_account.owner_key_id().ok_or_else(|| {
        invalid_params(
            "fee_account",
            Some("Fee account does not have an owner key set".to_string()),
        )
    })?;

    let transaction = builder
        .pay_fee_from_component(*fee_account.component_address(), req.max_fee)
        .with_min_epoch(req.min_epoch.map(Epoch))
        .with_max_epoch(req.max_epoch.map(Epoch))
        .build_unsigned();

    let request = TransactionSubmitRequest {
        transaction,
        seal_signer: owner_key_id,
        other_signers: vec![],
        detect_inputs: req.override_inputs.unwrap_or_default(),
        detect_inputs_use_unversioned: true,
        lock_ids: vec![],
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

    let mut transaction = context
        .transaction_builder()
        .with_unsigned_transaction(req.transaction)
        .with_inputs(detected_inputs)
        .finish();

    if !req.other_signers.is_empty() {
        let main_signer = sdk.key_manager_api().get_public_key(req.seal_signer)?;
        let main_signer_pk = main_signer.public_key.to_byte_type();
        let local_signer = sdk.signer_api().with_context(&main_signer_pk);
        for key in req.other_signers {
            transaction = local_signer.sign(key, transaction)?;
        }
    }

    let transaction = sdk.signer_api().sign(req.seal_signer, transaction)?;

    let tx_id = transaction.calculate_id();
    for lock_id in req.lock_ids {
        // update the locks with the corresponding transaction ID so that they can be finalized/released once the
        // transaction resolves
        sdk.transaction_api().locks_set_transaction_id(lock_id, tx_id)?;
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
        .map_err(map_transaction_submission_error)?;

    Ok(TransactionSubmitResponse { transaction_id })
}

fn map_transaction_submission_error(e: TransactionServiceError) -> anyhow::Error {
    error!(target: LOG_TARGET, "Transaction submission failed: {}", e);
    match &e {
        TransactionServiceError::TransactionApiError(TransactionApiError::NetworkInterfaceError { status, .. }) => {
            match &status {
                ResponseErrorStatus::TransactionRejected { message } => transaction_rejected(message),
                ResponseErrorStatus::NotFound { message } => not_found(message),
                ResponseErrorStatus::InternalError { message } => anyhow!("Failed to submit transaction: {}", message),
            }
        },
        _ => anyhow!("Failed to submit transaction: {}", e),
    }
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
    let key = key_api.get_public_key(req.seal_signer)?;

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
        .finish();
    let transaction = sdk.signer_api().sign(key.key_id, transaction)?;

    for lock_id in req.lock_ids {
        // update the proofs table with the corresponding transaction hash
        sdk.transaction_api()
            .locks_set_transaction_id(lock_id, transaction.calculate_id())?;
    }

    info!(
        target: LOG_TARGET,
        "Submitted transaction with hash {}",
        transaction.calculate_id()
    );
    let exec_result = context
        .transaction_service()
        .submit_dry_run_transaction(transaction)
        .await
        .map_err(map_transaction_submission_error)?;

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

    let default_account = sdk
        .accounts_api()
        .get_default()
        .optional()?
        .ok_or_else(|| invalid_request("No default account found".to_string()))?;

    let account_owner_key_id = default_account.owner_key_id().ok_or_else(|| {
        invalid_params(
            "signing_key_id",
            Some("Default account does not have an owner key set".to_string()),
        )
    })?;

    let signing_key_id = req.signing_key_id.unwrap_or(account_owner_key_id);

    let fee_amount = req.max_fee;

    let transaction = context
        .transaction_builder()
        .with_dry_run(req.dry_run)
        .with_fee_instructions_builder(|builder| {
            if instructions.fee_instructions.is_empty() {
                builder.call_method(*default_account.component_address(), "pay_fee", args![fee_amount])
            } else {
                builder.with_instructions(instructions.fee_instructions)
            }
        })
        .with_instructions(instructions.instructions)
        .build_unsigned();

    // Detect inputs
    let substates = transaction.to_referenced_substates()?.into_iter().collect::<Vec<_>>();
    let dependencies = sdk.substate_api().locate_dependent_substates(&substates, true).await?;
    let inputs = dependencies.into_iter().map(|input| input.into_unversioned());

    let transaction = transaction.with_inputs(inputs);

    let transaction = if signing_key_id == account_owner_key_id {
        transaction.finish()
    } else {
        sdk.signer_api()
            .with_context(default_account.owner_public_key())
            .sign(signing_key_id, transaction)?
    };

    let transaction = sdk.signer_api().sign(account_owner_key_id, transaction)?;

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
    let owner_key_id = fee_account.owner_key_id().ok_or_else(|| {
        invalid_params(
            "fee_account",
            Some("Fee account does not have an owner key set".to_string()),
        )
    })?;

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

    let wasm_binary = wasm_binary
        .try_into()
        .map_err(|_| invalid_params("binary", Some("WASM binary too large".to_string())))?;

    let metadata_hash = req.metadata.map(resolve_metadata_hash).transpose()?;

    let builder = context
        .transaction_builder()
        .pay_fee_from_component(*fee_account.component_address(), req.max_fee);
    let builder = match metadata_hash {
        Some(hash) => builder.publish_template_with_metadata(wasm_binary, hash),
        None => builder.publish_template(wasm_binary),
    };
    let transaction = builder.build_unsigned();

    if req.dry_run {
        let request = TransactionSubmitDryRunRequest {
            transaction,
            seal_signer: owner_key_id,
            other_signers: vec![],
            detect_inputs: req.detect_inputs,
            detect_inputs_use_unversioned: true,
            lock_ids: vec![],
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
        seal_signer: owner_key_id,
        other_signers: vec![],
        detect_inputs: req.detect_inputs,
        detect_inputs_use_unversioned: true,
        lock_ids: vec![],
    };
    let resp = handle_submit(context, token, request).await?;
    Ok(PublishTemplateResponse {
        transaction_id: resp.transaction_id,
        dry_run_fee: None,
    })
}

fn resolve_metadata_hash(
    input: tari_ootle_walletd_client::types::PublishTemplateMetadata,
) -> Result<tari_ootle_template_metadata::MetadataHash, anyhow::Error> {
    use tari_ootle_template_metadata::TemplateMetadata;
    use tari_ootle_walletd_client::types::PublishTemplateMetadata;

    match input {
        PublishTemplateMetadata::Hash(hash) => Ok(hash),
        PublishTemplateMetadata::Literal(meta) => meta.hash().map_err(|e| anyhow!(e)),
        PublishTemplateMetadata::RawCbor(bytes) => {
            let meta = TemplateMetadata::from_cbor(&bytes).map_err(|e| anyhow!(e))?;
            meta.hash().map_err(|e| anyhow!(e))
        },
    }
}
