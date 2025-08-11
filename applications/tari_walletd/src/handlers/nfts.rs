//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, slice};

use anyhow::anyhow;
use axum::headers::authorization::Bearer;
use log::{info, warn};
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    json_cbor::convert_json_to_cbor,
    substate::SubstateId,
    ToByteType,
};
use tari_ootle_common_types::{optional::Optional, SubstateRequirement};
use tari_ootle_wallet_sdk::apis::substate::ValidatorScanResult;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    constants::{NFT_FAUCET_COMPONENT_ADDRESS, NFT_FAUCET_RESOURCE_ADDRESS},
    models::{ComponentAddress, ResourceAddress},
};
use tari_transaction::{args, TransactionId};
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        GetNftRequest,
        GetNftResponse,
        ListNftsRequest,
        ListNftsResponse,
        MintFaucetNftRequest,
        MintFaucetNftResponse,
        TransferNftRequest,
        TransferNftResponse,
    },
};
use tokio::sync::broadcast;

use super::{context::HandlerContext, helpers::get_account_or_default};
use crate::{
    handlers::helpers::{application_error, get_account, get_account_with_inputs, invalid_params},
    jrpc_server::ApplicationErrorCode,
    services::{TransactionFinalizedEvent, WalletEvent},
    DEFAULT_FEE,
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::nfts";

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: GetNftRequest,
) -> Result<GetNftResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let non_fungible_api = sdk.non_fungible_api();

    let non_fungible = non_fungible_api
        .get(req.resource_address, req.nft_id)
        .map_err(|e| anyhow!("Failed to get non fungible token, with error: {}", e))?;

    Ok(non_fungible)
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ListNftsRequest,
) -> Result<ListNftsResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let ListNftsRequest { account, limit, offset } = req;
    let sdk = context.wallet_sdk();
    let account = get_account_or_default(account.as_ref(), &sdk.accounts_api())?;
    let account = account.account;

    let non_fungible_api = sdk.non_fungible_api();

    let non_fungibles = non_fungible_api
        .get_all(account.address, limit, offset)
        .map_err(|e| anyhow!("Failed to list all non fungibles, with error: {}", e))?;
    Ok(ListNftsResponse { nfts: non_fungibles })
}

pub async fn handle_mint_faucet(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: MintFaucetNftRequest,
) -> Result<MintFaucetNftResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    let key_manager_api = sdk.key_manager_api();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let account = get_account(&req.account, &sdk.accounts_api())?;
    let account = account.account;

    let signing_key = key_manager_api.derive_account_key(account.key_index)?;

    info!(target: LOG_TARGET, "🎮 Minting new NFT with metadata {}", req.mutable_data);

    let mutable_data = convert_json_to_cbor(req.mutable_data).map_err(|e| invalid_params("mutable_data", Some(e)))?;

    if req.number_to_mint == 0 {
        return Err(invalid_params("number_to_mint", Some("number_to_mint is zero")));
    }

    let inputs = sdk
        .substate_api()
        .locate_dependent_substates(slice::from_ref(&account.address.into()), true)
        .await?;
    let fee = req.max_fee.unwrap_or(DEFAULT_FEE);
    let transaction = context
        .transaction_builder()
        .fee_transaction_pay_from_component(account.address, fee)
        .call_method(NFT_FAUCET_COMPONENT_ADDRESS, "mint", args![
            Amount(req.number_to_mint),
            mutable_data
        ])
        .put_last_instruction_output_on_workspace("tokens")
        .call_method(account.address, "deposit", args![Workspace("tokens")])
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .add_input(NFT_FAUCET_COMPONENT_ADDRESS)
        .add_input(NFT_FAUCET_RESOURCE_ADDRESS)
        .build_and_seal(&signing_key.key);

    let mut events = context.notifier().subscribe();
    let tx_id = context.transaction_service().submit_transaction(transaction).await?;

    let finalize_event = wait_for_result(&mut events, tx_id).await?;
    if let Some(reject) = finalize_event.finalize.any_reject() {
        return Err(application_error(
            ApplicationErrorCode::TransactionRejected,
            format!("Mint new NFT using account {} was rejected: {}", account, reject),
        ));
    }

    Ok(MintFaucetNftResponse {
        transaction_id: tx_id,
        finalize: finalize_event.finalize,
        fee: finalize_event.final_fee,
    })
}

async fn try_find_target_account(
    context: &HandlerContext,
    inputs: &mut HashSet<SubstateRequirement>,
    target_account_address: ComponentAddress,
    target_resource_address: ResourceAddress,
) -> anyhow::Result<bool> {
    let sdk = context.wallet_sdk();
    let existing_account = sdk
        .substate_api()
        .scan_for_substate(&SubstateId::Component(target_account_address), None)
        .await
        .optional()?;

    let Some(ValidatorScanResult { address, substate }) = existing_account else {
        return Ok(false);
    };
    inputs.insert(address.into());

    // Figure out which vault to add as an input
    let Some(component) = substate.component() else {
        return Err(anyhow::anyhow!(
            "The target account {} is not a component. This is unexpected.",
            target_account_address
        ));
    };
    let indexed = component.body.to_indexed_well_known_types()?;
    let mut found_dest_vault = None;
    for vault_id in indexed.vault_ids() {
        // Local vault?
        match sdk.accounts_api().get_vault(vault_id).optional()? {
            Some(vault) => {
                if vault.resource_address != target_resource_address {
                    // Continue searching for a vault for the resource address
                    continue;
                }
                // Found it - we're sending to our own vault
                found_dest_vault = Some(*vault_id);
                break;
            },
            None => {
                // TODO(perf): slow with lots of vaults
                let vault = sdk
                    .substate_api()
                    .scan_for_substate(&SubstateId::Vault(*vault_id), None)
                    .await
                    .optional()?;

                let Some(vault) = vault.and_then(|scan| scan.substate.into_vault()) else {
                    warn!(
                        target: LOG_TARGET,
                        "❓️ The target account {target_account_address} contains a vault {vault_id} that was not found. This is unexpected.",
                    );
                    continue;
                };

                if *vault.resource_address() != target_resource_address {
                    // Continue searching for a vault for the resource address
                    continue;
                }

                // Found it
                found_dest_vault = Some(*vault_id);
                break;
            },
        }
    }

    if let Some(found) = found_dest_vault {
        inputs.insert(SubstateRequirement::unversioned(found));
    }

    Ok(true)
}

#[allow(clippy::too_many_lines)]
pub async fn handle_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransferNftRequest,
) -> Result<TransferNftResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    // fetch accounts and its inputs
    let (fee_payer_account, fee_payer_account_inputs) = get_account_with_inputs(Some(&req.fee_payer_account), sdk)?;
    let fee_payer_account = fee_payer_account.account;
    let fee_payer_account_address = fee_payer_account.address;
    let (source_account, mut inputs) = get_account_with_inputs(Some(&req.source_account), sdk)?;
    inputs.extend(fee_payer_account_inputs);
    let source_account_address = *source_account.address();

    let target_account_address =
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &req.target_account_public_key);

    // TODO: this can be simplified
    let mut builder = context.transaction_builder();
    // collect all instructions
    let non_fungible_api = sdk.non_fungible_api();

    if !try_find_target_account(context, &mut inputs, target_account_address, req.resource_address).await? {
        // We need to create the target account
        builder = builder.create_account(req.target_account_public_key)
    }
    // add the input for the source account vault substate
    let src_vault = sdk
        .accounts_api()
        .get_vault_by_resource(source_account.address(), &req.resource_address)?;
    let src_vault_substate = sdk.substate_api().get_substate(&src_vault.id.into())?;
    inputs.insert(src_vault_substate.substate_id.into());
    inputs.insert(SubstateRequirement::unversioned(src_vault.resource_address));

    for (i, nft_id) in req.nfts.into_iter().enumerate() {
        // Check if the NFT is owned by this wallet
        let nft = non_fungible_api
            .get(req.resource_address, nft_id.clone())
            .optional()
            .map_err(|e| anyhow!("Failed to get non-fungible token: {}", e))?;
        if nft.is_none() {
            return Err(invalid_params(
                "nft_id",
                Some(format!(
                    "NFT with ID {} not found for resource {}",
                    nft_id, req.resource_address
                )),
            ));
        }

        builder = builder
            .call_method(source_account_address, "withdraw_non_fungible", args![
                req.resource_address,
                nft_id,
            ])
            .put_last_instruction_output_on_workspace(format!("b-{i}"))
            .call_method(target_account_address, "deposit", args![Workspace(format!("b-{i}"))]);
    }

    let (fee_payer_account_secret_key, fee_payer_account_public_key) = sdk
        .key_manager_api()
        .derive_account_keypair(fee_payer_account.key_index)?;

    let source_account_secret_key = sdk.key_manager_api().derive_account_key(source_account.key_index())?;

    let transaction = builder
        .with_dry_run(req.dry_run)
        .fee_transaction_pay_from_component(fee_payer_account_address, req.max_fee)
        .with_inputs(inputs)
        // Seal signer is the fee payer account
        .with_authorized_seal_signer()
        .add_signature(
            &fee_payer_account_public_key.to_byte_type(),
            &source_account_secret_key.key,
        )
        .build_and_seal(&fee_payer_account_secret_key.key);

    // if dry run, we can return the result immediately
    if req.dry_run {
        let execute_result = context
            .transaction_service()
            .submit_dry_run_transaction(transaction)
            .await?;
        let transaction_id = execute_result.finalize.transaction_hash.into();
        let finalize = execute_result.finalize;
        return Ok(TransferNftResponse {
            transaction_id,
            // TODO: this could cause a crash, change api to use u64
            fee: finalize.fee_receipt.total_fees_paid,
            fee_refunded: finalize.fee_receipt.total_fee_payment - finalize.fee_receipt.total_fees_paid,
            result: finalize,
        });
    }

    // execute transaction
    let mut events = context.notifier().subscribe();
    let tx_id = context.transaction_service().submit_transaction(transaction).await?;

    let finalized = crate::handlers::helpers::wait_for_result(&mut events, tx_id).await?;

    if let Some(reject) = finalized.finalize.result.fee_reject() {
        return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
    }
    if let Some(reason) = finalized.finalize.any_reject() {
        return Err(anyhow::anyhow!(
            "Fee transaction succeeded (fees charged) however the transaction failed: {}",
            reason
        ));
    }
    info!(
        target: LOG_TARGET,
        "✅ Transferring NFT transaction {} finalized. Fee: {}",
        finalized.transaction_id,
        finalized.final_fee
    );

    Ok(TransferNftResponse {
        transaction_id: tx_id,
        fee: finalized.final_fee,
        fee_refunded: req.max_fee - finalized.final_fee,
        result: finalized.finalize,
    })
}

async fn wait_for_result(
    events: &mut broadcast::Receiver<WalletEvent>,
    transaction_id: TransactionId,
) -> Result<TransactionFinalizedEvent, anyhow::Error> {
    loop {
        let wallet_event = events.recv().await?;
        match wallet_event {
            WalletEvent::TransactionFinalized(event) if event.transaction_id == transaction_id => return Ok(event),
            _ => {},
        }
    }
}
