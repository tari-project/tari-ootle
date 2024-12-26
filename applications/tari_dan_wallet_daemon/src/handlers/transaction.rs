//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use std::time::Duration;

use anyhow::anyhow;
use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
use futures::{future, future::Either};
use log::*;
use tari_dan_app_utilities::json_encoding;
use tari_dan_common_types::{optional::Optional, Epoch, SubstateRequirement};
use tari_dan_wallet_sdk::apis::{jwt::JrpcPermission, key_manager};
use tari_template_lib::{args, models::Amount};
use tari_transaction::Transaction;
use tari_wallet_daemon_client::types::{
    AccountGetRequest,
    AccountGetResponse,
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
    TransactionSubmitRequest,
    TransactionSubmitResponse,
    TransactionWaitResultRequest,
    TransactionWaitResultResponse,
};
use tokio::time;

use super::{accounts, context::HandlerContext};
use crate::{
    handlers::{helpers::get_account_or_default, HandlerError},
    services::WalletEvent,
};

const LOG_TARGET: &str = "tari::dan::wallet_daemon::handlers::transaction";

pub async fn handle_submit_instruction(
    context: &HandlerContext,
    token: Option<String>,
    req: CallInstructionRequest,
) -> Result<TransactionSubmitResponse, anyhow::Error> {
    let mut builder = Transaction::builder().with_instructions(req.instructions);

    if let Some(dump_account) = req.dump_outputs_into {
        let AccountGetResponse {
            account: dump_account, ..
        } = accounts::handle_get(context, token.clone(), AccountGetRequest {
            name_or_address: dump_account,
        })
        .await?;

        builder = builder.put_last_instruction_output_on_workspace("bucket").call_method(
            dump_account.address.as_component_address().unwrap(),
            "deposit",
            args![Variable("bucket")],
        );
    }
    let AccountGetResponse {
        account: fee_account, ..
    } = accounts::handle_get(context, token.clone(), AccountGetRequest {
        name_or_address: req.fee_account,
    })
    .await?;

    let transaction = builder
        .fee_transaction_pay_from_component(
            fee_account.address.as_component_address().unwrap(),
            req.max_fee.try_into()?,
        )
        .with_min_epoch(req.min_epoch.map(Epoch))
        .with_max_epoch(req.max_epoch.map(Epoch))
        .build_unsigned_transaction();

    let request = TransactionSubmitRequest {
        transaction,
        signing_key_index: Some(fee_account.key_index),
        autofill_inputs: vec![],
        detect_inputs: req.override_inputs.unwrap_or_default(),
        detect_inputs_use_unversioned: false,
        proof_ids: vec![],
    };
    handle_submit(context, token, request).await
}

pub async fn handle_submit(
    context: &HandlerContext,
    token: Option<String>,
    req: TransactionSubmitRequest,
) -> Result<TransactionSubmitResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    // TODO: fine-grained checks of individual addresses involved (resources, components, etc)
    sdk.jwt_api()
        .check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let key_api = sdk.key_manager_api();
    // Fetch the key to sign the transaction
    // TODO: Ideally the SDK should take care of signing the transaction internally
    let (_, key) = key_api.get_key_or_active(key_manager::TRANSACTION_BRANCH, req.signing_key_index)?;

    let autofill_inputs = req.autofill_inputs;
    let detected_inputs = if req.detect_inputs {
        // If we are not overriding inputs, we will use inputs that we know about in the local substate id db
        let substates = req.transaction.to_referenced_substates()?;
        let substates = substates.into_iter().collect::<Vec<_>>();
        let loaded_substates = sdk.substate_api().locate_dependent_substates(&substates).await?;
        loaded_substates
            .into_iter()
            .chain(substates.into_iter().map(SubstateRequirement::unversioned))
            .map(|mut input| {
                if req.detect_inputs_use_unversioned {
                    input.version = None;
                }
                input
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

    let transaction = Transaction::builder()
        .with_unsigned_transaction(req.transaction)
        .with_inputs(detected_inputs)
        .build_and_seal(&key.key);

    for input in transaction.inputs() {
        debug!(target: LOG_TARGET, "Input: {}", input)
    }

    for proof_id in req.proof_ids {
        // update the proofs table with the corresponding transaction hash
        sdk.confidential_outputs_api()
            .proofs_set_transaction_hash(proof_id, *transaction.id())?;
    }

    info!(
        target: LOG_TARGET,
        "Submitted transaction with hash {}",
        transaction.hash()
    );

    let transaction_id = context
        .transaction_service()
        .submit_transaction(transaction, autofill_inputs)
        .await?;

    Ok(TransactionSubmitResponse { transaction_id })
}

pub async fn handle_submit_dry_run(
    context: &HandlerContext,
    token: Option<String>,
    req: TransactionSubmitDryRunRequest,
) -> Result<TransactionSubmitDryRunResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    // TODO: fine-grained checks of individual addresses involved (resources, components, etc)
    sdk.jwt_api()
        .check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let key_api = sdk.key_manager_api();
    // Fetch the key to sign the transaction
    // TODO: Ideally the SDK should take care of signing the transaction internally
    let (_, key) = key_api.get_key_or_active(key_manager::TRANSACTION_BRANCH, req.signing_key_index)?;

    let autofill_inputs = req.autofill_inputs;
    let detected_inputs = if req.detect_inputs {
        // If we are not overriding inputs, we will use inputs that we know about in the local substate id db
        let substates = req.transaction.to_referenced_substates()?;
        let substates = substates.into_iter().collect::<Vec<_>>();
        sdk.substate_api().locate_dependent_substates(&substates).await?
    } else {
        vec![]
    };

    let transaction = Transaction::builder()
        .with_unsigned_transaction(req.transaction)
        .with_inputs(detected_inputs)
        .build_and_seal(&key.key);

    for proof_id in req.proof_ids {
        // update the proofs table with the corresponding transaction hash
        sdk.confidential_outputs_api()
            .proofs_set_transaction_hash(proof_id, *transaction.id())?;
    }

    info!(
        target: LOG_TARGET,
        "Submitted transaction with hash {}",
        transaction.hash()
    );
    let exec_result = context
        .transaction_service()
        .submit_dry_run_transaction(transaction, autofill_inputs)
        .await?;

    let json_result = json_encoding::encode_finalize_result_into_json(&exec_result.finalize)?;

    Ok(TransactionSubmitDryRunResponse {
        transaction_id: exec_result.finalize.transaction_hash.into_array().into(),
        result: exec_result,
        json_result,
    })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<String>,
    req: TransactionGetRequest,
) -> Result<TransactionGetResponse, anyhow::Error> {
    context
        .wallet_sdk()
        .jwt_api()
        .check_auth(token, &[JrpcPermission::TransactionGet])?;
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
        last_update_time: transaction.last_update_time,
    })
}

pub async fn handle_get_all(
    context: &HandlerContext,
    token: Option<String>,
    req: TransactionGetAllRequest,
) -> Result<TransactionGetAllResponse, anyhow::Error> {
    context
        .wallet_sdk()
        .jwt_api()
        .check_auth(token, &[JrpcPermission::TransactionGet])?;
    let transactions = context
        .wallet_sdk()
        .transaction_api()
        .fetch_all(req.status, req.component)?;
    Ok(TransactionGetAllResponse {
        transactions: transactions
            .into_iter()
            .map(|tx| (tx.transaction, tx.finalize, tx.status, tx.last_update_time))
            .collect(),
    })
}

pub async fn handle_get_result(
    context: &HandlerContext,
    token: Option<String>,
    req: TransactionGetResultRequest,
) -> Result<TransactionGetResultResponse, anyhow::Error> {
    context
        .wallet_sdk()
        .jwt_api()
        .check_auth(token, &[JrpcPermission::TransactionGet])?;
    let transaction = context
        .wallet_sdk()
        .transaction_api()
        .get(req.transaction_id)
        .optional()?
        .ok_or(HandlerError::NotFound)?;

    let json_result = transaction
        .finalize
        .as_ref()
        .map(json_encoding::encode_finalize_result_into_json)
        .transpose()?;

    Ok(TransactionGetResultResponse {
        transaction_id: req.transaction_id,
        result: transaction.finalize,
        status: transaction.status,
        json_result,
    })
}

pub async fn handle_wait_result(
    context: &HandlerContext,
    token: Option<String>,
    req: TransactionWaitResultRequest,
) -> Result<TransactionWaitResultResponse, anyhow::Error> {
    context
        .wallet_sdk()
        .jwt_api()
        .check_auth(token, &[JrpcPermission::TransactionGet])?;
    let mut events = context.notifier().subscribe();
    let transaction = context
        .wallet_sdk()
        .transaction_api()
        .get(req.transaction_id)
        .optional()?
        .ok_or(HandlerError::NotFound)?;

    if let Some(result) = transaction.finalize {
        let json_result = json_encoding::encode_finalize_result_into_json(&result)?;

        return Ok(TransactionWaitResultResponse {
            transaction_id: req.transaction_id,
            result: Some(result),
            status: transaction.status,
            final_fee: transaction.final_fee.unwrap_or_default(),
            timed_out: false,
            json_result: Some(json_result),
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
                let json_result = json_encoding::encode_finalize_result_into_json(&event.finalize)?;
                return Ok(TransactionWaitResultResponse {
                    transaction_id: req.transaction_id,
                    result: Some(event.finalize),
                    status: event.status,
                    final_fee: event.final_fee,
                    timed_out: false,
                    json_result: Some(json_result),
                });
            },
            Some(WalletEvent::TransactionInvalid(event)) if event.transaction_id == req.transaction_id => {
                return Ok(TransactionWaitResultResponse {
                    transaction_id: req.transaction_id,
                    result: event.finalize,
                    status: event.status,
                    final_fee: event.final_fee.unwrap_or_default(),
                    timed_out: false,
                    json_result: None,
                });
            },
            Some(_) => continue,
            None => {
                return Ok(TransactionWaitResultResponse {
                    transaction_id: req.transaction_id,
                    result: None,
                    status: transaction.status,
                    final_fee: Amount::zero(),
                    timed_out: true,
                    json_result: None,
                });
            },
        };
    }
}

pub async fn handle_publish_template(
    context: &HandlerContext,
    token: Option<String>,
    req: PublishTemplateRequest,
) -> Result<PublishTemplateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();

    let fee_account = get_account_or_default(req.fee_account, &sdk.accounts_api())?;

    let transaction = Transaction::builder()
        .fee_transaction_pay_from_component(
            fee_account.address.as_component_address().unwrap(),
            req.max_fee.try_into()?,
        )
        .publish_template(req.binary)
        .build_unsigned_transaction();

    if req.dry_run {
        let request = TransactionSubmitDryRunRequest {
            transaction,
            signing_key_index: Some(fee_account.key_index),
            autofill_inputs: vec![],
            detect_inputs: req.detect_inputs,
            proof_ids: vec![],
        };
        let resp = handle_submit_dry_run(context, token, request).await?;
        if let Some(reject) = resp.result.finalize.full_reject() {
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
        signing_key_index: Some(fee_account.key_index),
        autofill_inputs: vec![],
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
