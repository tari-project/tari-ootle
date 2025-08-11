//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum::headers::authorization::Bearer;
use tari_ootle_wallet_sdk::apis::stealth_transfer::StealthTransferParams;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{StealthTransferRequest, StealthTransferResponse},
};
use tokio::task;

use crate::{
    handlers::{
        helpers::{get_account, invalid_params},
        HandlerContext,
    },
    services::TransactionSubmittedEvent,
};

pub async fn handle_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthTransferRequest,
) -> Result<StealthTransferResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk().clone();
    let notifier = context.notifier().clone();
    let owner_account = get_account(&req.owner_account, &sdk.accounts_api())?;

    let params = StealthTransferParams {
        owner_account,
        revealed_to_account: req.revealed_to_account,
        input_selection: req.input_selection,
        destination_public_key: req.destination_public_key,
        resource_address: req.resource_address,
        max_fee: req.max_fee,
        blinded_output_amount: req.blinded_output_amount,
        revealed_output_amount: req.revealed_output_amount,
        is_dry_run: req.dry_run,
    };
    if let Err(err) = params.validate() {
        return Err(invalid_params("params", Some(err)));
    }

    let transaction_service = context.transaction_service().clone();

    // Spawn here is to prevent the async block from being aborted if the caller aborts the request early as this can
    // cause funds to remain locked indefinitely.
    task::spawn(async move {
        let transfer = sdk.stealth_transfer_api().transfer(params).await?;

        if req.dry_run {
            let transaction_id = transfer.transaction.calculate_id();
            transaction_service
                .submit_dry_run_transaction(transfer.transaction)
                .await?;
            return Ok(StealthTransferResponse { transaction_id });
        }

        // let mut events = notifier.subscribe();
        let tx_id = transaction_service.submit_transaction(transfer.transaction).await?;

        notifier.notify(TransactionSubmittedEvent {
            transaction_id: tx_id,
            new_account: None,
        });

        // let finalized = wait_for_result(&mut events, tx_id).await?;
        // if let Some(reject) = finalized.finalize.result.fee_reject() {
        //     return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
        // }
        // if let Some(reason) = finalized.finalize.fee_reject() {
        //     return Err(anyhow::anyhow!(
        //         "Fee transaction succeeded (fees charged) however the transaction failed: {}",
        //         reason
        //     ));
        // }

        Ok(StealthTransferResponse {
            transaction_id: tx_id,
            // fee: finalized.final_fee,
            // result: finalized.finalize,
        })
    })
    .await?
}
