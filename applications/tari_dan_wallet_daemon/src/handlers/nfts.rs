//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, str::FromStr};

use super::{context::HandlerContext, helpers::get_account_or_default};
use crate::{
    handlers::helpers::{application_error, get_account, transaction_builder},
    jrpc_server::ApplicationErrorCode,
    services::{TransactionFinalizedEvent, WalletEvent},
    DEFAULT_FEE,
};
use anyhow::anyhow;
use log::info;
use tari_crypto::{
    keys::PublicKey as PK,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_dan_common_types::optional::Optional;
use tari_dan_common_types::SubstateRequirement;
use tari_dan_wallet_sdk::{
    apis::{jwt::JrpcPermission, key_manager},
    models::Account,
};
use tari_engine_types::{
    component::new_component_address_from_public_key,
    instruction::Instruction,
    substate::SubstateId,
};
use tari_template_builtin::{ACCOUNT_NFT_TEMPLATE_ADDRESS, ACCOUNT_TEMPLATE_ADDRESS};
use tari_template_lib::{
    args,
    models::{Amount, ComponentAddress, Metadata, NonFungibleAddress, NonFungibleId, ResourceAddress},
    types::crypto::RistrettoPublicKeyBytes,
};
use tari_transaction::TransactionId;
use tari_wallet_daemon_client::{
    types::{
        AccountsTransferResponse,
        GetAccountNftRequest,
        GetAccountNftResponse,
        ListAccountNftRequest,
        ListAccountNftResponse,
        MintAccountNftRequest,
        MintAccountNftResponse,
        TransferNftRequest,
        TransferNftResponse,
    },
    ComponentAddressOrName,
};
use tokio::sync::broadcast;

const LOG_TARGET: &str = "tari::dan::wallet_daemon::handlers::nfts";

pub async fn handle_get_nft(
    context: &HandlerContext,
    token: Option<String>,
    req: GetAccountNftRequest,
) -> Result<GetAccountNftResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    let non_fungible_api = sdk.non_fungible_api();

    let non_fungible = non_fungible_api
        .get_by_id(req.nft_id)
        .map_err(|e| anyhow!("Failed to get non fungible token, with error: {}", e))?;

    Ok(non_fungible)
}

pub async fn handle_list_nfts(
    context: &HandlerContext,
    token: Option<String>,
    req: ListAccountNftRequest,
) -> Result<ListAccountNftResponse, anyhow::Error> {
    let ListAccountNftRequest { account, limit, offset } = req;
    let sdk = context.wallet_sdk();
    let account = get_account_or_default(account, &sdk.accounts_api())?;
    let sdk = context.wallet_sdk();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    let non_fungible_api = sdk.non_fungible_api();

    let non_fungibles = non_fungible_api
        .get_all(account.address.as_component_address().unwrap(), limit, offset)
        .map_err(|e| anyhow!("Failed to list all non fungibles, with error: {}", e))?;
    Ok(ListAccountNftResponse { nfts: non_fungibles })
}

pub async fn handle_mint_account_nft(
    context: &HandlerContext,
    token: Option<String>,
    req: MintAccountNftRequest,
) -> Result<MintAccountNftResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    let key_manager_api = sdk.key_manager_api();
    sdk.jwt_api().check_auth(token.clone(), &[JrpcPermission::Admin])?;

    let account = get_account(&req.account, &sdk.accounts_api())?;

    let signing_key_index = account.key_index;
    let signing_key = key_manager_api.derive_key(key_manager::TRANSACTION_BRANCH, signing_key_index)?;

    let owner_pk = RistrettoPublicKey::from_secret_key(&signing_key.key);
    let owner_token =
        NonFungibleAddress::from_public_key(RistrettoPublicKeyBytes::from_bytes(owner_pk.as_bytes()).unwrap());

    info!(target: LOG_TARGET, "Minting new NFT with metadata {}", req.metadata);

    let mut total_fee = Amount::new(0);
    let component_address = match req.existing_nft_component {
        Some(existing_nft_component) => existing_nft_component,
        None => {
            let resp = create_account_nft(
                context,
                &account,
                &signing_key.key,
                owner_token,
                req.create_account_nft_fee.unwrap_or(DEFAULT_FEE),
                token.clone(),
            )
            .await?;

            total_fee += resp.final_fee;
            if let Some(reason) = resp.finalize.result.any_reject() {
                return Err(anyhow!("Failed to create account NFT: {}", reason));
            }
            let component_address = resp
                .finalize
                .result
                .accept()
                .unwrap()
                .up_iter()
                .filter(|(id, _)| id.is_component())
                .find(|(_, s)| s.substate_value().component().unwrap().template_address == ACCOUNT_NFT_TEMPLATE_ADDRESS)
                .map(|(id, _)| id.as_component_address().unwrap())
                .ok_or_else(|| anyhow!("Failed to find account NFT component address"))?;

            // Strange issue with current rust version, if return the _OWNED_ value directly, it will not compile.
            #[allow(clippy::let_and_return)]
            component_address
        },
    };

    let metadata = Metadata::from(serde_json::from_value::<BTreeMap<String, String>>(req.metadata)?);

    let resp = mint_account_nft(
        context,
        token,
        account,
        component_address,
        &signing_key.key,
        req.mint_fee.unwrap_or(DEFAULT_FEE),
        metadata,
    )
    .await?;
    // TODO: is there a more direct way to extract nft_id and resource address ??
    let (resource_address, nft_id) = resp
        .finalize
        .events
        .iter()
        .find(|e| e.topic().as_str() == "mint")
        .map(|e| {
            (
                e.get_payload("resource_address").expect("Resource address not found"),
                e.get_payload("id").expect("NFTID not found"),
            )
        })
        .expect("NFT ID event payload not found");
    let resource_address = ResourceAddress::from_str(&resource_address)?;
    let nft_id = NonFungibleId::try_from_canonical_string(nft_id.as_str())
        .map_err(|e| anyhow!("Failed to parse non fungible id, with error: {:?}", e))?;

    total_fee += resp.final_fee;

    Ok(MintAccountNftResponse {
        result: resp.finalize,
        resource_address,
        nft_id,
        fee: total_fee,
    })
}

async fn mint_account_nft(
    context: &HandlerContext,
    token: Option<String>,
    account: Account,
    component_address: ComponentAddress,
    owner_sk: &RistrettoSecretKey,
    fee: Amount,
    metadata: Metadata,
) -> Result<TransactionFinalizedEvent, anyhow::Error> {
    let sdk = context.wallet_sdk();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    let inputs = sdk
        .substate_api()
        .locate_dependent_substates(&[account.address.clone(), component_address.into()])
        .await?;

    let transaction = transaction_builder(context)
        .fee_transaction_pay_from_component(account.address.as_component_address().unwrap(), fee)
        .call_method(component_address, "mint", args![metadata])
        .put_last_instruction_output_on_workspace(b"bucket".to_vec())
        .call_method(account.address.as_component_address().unwrap(), "deposit", args![
            Workspace("bucket")
        ])
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .build_and_seal(owner_sk);

    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction(transaction, vec![])
        .await?;

    let event = wait_for_result(&mut events, tx_id).await?;
    if let Some(reject) = event.finalize.any_reject() {
        return Err(application_error(
            ApplicationErrorCode::TransactionRejected,
            format!("Mint new NFT using account {} was rejected: {}", account, reject),
        ));
    }

    Ok(event)
}

async fn create_account_nft(
    context: &HandlerContext,
    account: &Account,
    owner_sk: &RistrettoSecretKey,
    owner_token: NonFungibleAddress,
    fee: Amount,
    token: Option<String>,
) -> Result<TransactionFinalizedEvent, anyhow::Error> {
    let sdk = context.wallet_sdk();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    let inputs = sdk
        .substate_api()
        .locate_dependent_substates(&[account.address.clone()])
        .await?;

    let transaction = transaction_builder(context)
        .fee_transaction_pay_from_component(account.address.as_component_address().unwrap(), fee)
        .call_function(ACCOUNT_NFT_TEMPLATE_ADDRESS, "create", args![owner_token,])
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .build_and_seal(owner_sk);

    let tx_id = sdk
        .transaction_api()
        .insert_new_transaction(transaction, vec![], None, false)
        .await?;
    let mut events = context.notifier().subscribe();
    sdk.transaction_api().submit_transaction(tx_id).await?;

    let event = wait_for_result(&mut events, tx_id).await?;

    if let Some(reason) = event.finalize.fee_reject() {
        return Err(application_error(
            ApplicationErrorCode::TransactionRejected,
            format!(
                "Create NFT resource address transaction, from account {}, failed: {}",
                account, reason
            ),
        ));
    }

    Ok(event)
}

pub async fn handle_transfer_nft(
    context: &HandlerContext,
    token: Option<String>,
    req: TransferNftRequest,
) -> Result<TransferNftResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    info!(target: LOG_TARGET, "Received transfer nft request: {:?}", req);

    // get source/target accounts
    info!(target: LOG_TARGET, "Get source account...");
    let source_account = match req.source_account {
        ComponentAddressOrName::ComponentAddress(address) => sdk
            .accounts_api()
            .get_account_by_address(&SubstateId::Component(address)),
        ComponentAddressOrName::Name(name) => sdk.accounts_api().get_account_by_name(name.as_str()),
    }
    .map_err(|error| anyhow!("Failed to get source account: {error}"))?;

    info!(target: LOG_TARGET, "Found source account: {:?}", source_account);

    let source_account_address = source_account
        .address
        .as_component_address()
        .ok_or(anyhow!("Source account address is not a component address!"))?;

    info!(target: LOG_TARGET, "Found source account address: {:?}", source_account_address);

    let target_account_address =
        new_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &req.target_account_public_key);

    let existing_dest_account = sdk
        .substate_api()
        .scan_for_substate(&SubstateId::Component(target_account_address), None)
        .await
        .optional()?;

    info!(target: LOG_TARGET, "Found target account address: {:?}", target_account_address);

    // collect all instructions
    let mut instructions = vec![];
    let mut inputs = vec![];
    let non_fungible_api = sdk.non_fungible_api();
    for nft_id in req.nft_ids {
        // get NFT
        info!(target: LOG_TARGET, "Getting NFT: {:?}", nft_id);

        let nft = non_fungible_api
            .get_by_id(nft_id)
            .map_err(|e| anyhow!("Failed to get non fungible token: {}", e))?;

        info!(target: LOG_TARGET, "Got NFT: {:?}", nft);

        // add the input for the source account vault substate
        let src_vault = sdk
            .accounts_api()
            .get_vault_by_resource(&source_account.address, &nft.resource_address)?;
        let src_vault_substate = sdk.substate_api().get_substate(&src_vault.address)?;
        inputs.push(src_vault_substate.substate_id.into());
        let resource_substate_address = SubstateRequirement::unversioned(src_vault.resource_address);
        inputs.push(resource_substate_address.clone());

        instructions.extend([
            Instruction::CallMethod {
                component_address: source_account_address,
                method: "withdraw".to_string(),
                args: args![nft.resource_address, Amount::new(1)],
            },
            Instruction::PutLastInstructionOutputOnWorkspace {
                key: b"bucket".to_vec(),
            },
            Instruction::CallMethod {
                component_address: target_account_address,
                method: "deposit".to_string(),
                args: args![Workspace("bucket")],
            },
        ])
    }

    let source_account_secret_key = sdk
        .key_manager_api()
        .derive_key(key_manager::TRANSACTION_BRANCH, source_account.key_index)?;

    info!(target: LOG_TARGET, "Source account secret: {:?}", source_account_secret_key);

    let transaction = transaction_builder(context)
        .with_fee_instructions(vec![Instruction::CallMethod {
            component_address: source_account_address,
            method: "pay_fee".to_string(),
            args: args![req.max_fee],
        }])
        .with_instructions(instructions)
        .with_inputs(inputs)
        .build_and_seal(&source_account_secret_key.key);

    info!(target: LOG_TARGET, "Transaction built: {:?}", transaction);

    // if dry run, we can return the result immediately
    if req.dry_run {
        let transaction_id = *transaction.id();

        info!(target: LOG_TARGET, "Before execute dry run tx: {:?}", transaction_id);

        let execute_result = context
            .transaction_service()
            .submit_dry_run_transaction(transaction, vec![])
            .await?;

        info!(target: LOG_TARGET, "After execute dry run tx: {:?}", execute_result);

        let finalize = execute_result.finalize;
        return Ok(TransferNftResponse {
            transaction_id,
            fee: finalize.fee_receipt.total_fees_paid,
            fee_refunded: finalize.fee_receipt.total_fee_payment - finalize.fee_receipt.total_fees_paid,
            result: finalize,
        });
    }

    // execute transaction
    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction(transaction, vec![])
        .await?;

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
