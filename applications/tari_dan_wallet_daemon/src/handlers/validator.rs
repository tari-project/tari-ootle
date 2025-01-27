//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use anyhow::anyhow;
use either::Either;
use log::*;
use tari_common_types::types::PublicKey;
use tari_crypto::{keys::PublicKey as _, tari_utilities::ByteArray};
use tari_dan_common_types::{derive_fee_pool_address, optional::Optional, SubstateRequirement};
use tari_dan_wallet_crypto::byte_utils;
use tari_dan_wallet_sdk::apis::{jwt::JrpcPermission, key_manager};
use tari_engine_types::substate::SubstateId;
use tari_template_lib::args;
use tari_wallet_daemon_client::types::{
    AccountOrKeyIndex,
    ClaimValidatorFeesRequest,
    ClaimValidatorFeesResponse,
    FeePoolDetails,
    GetValidatorFeesRequest,
    GetValidatorFeesResponse,
};

use crate::{
    handlers::{
        helpers::{
            get_account_or_default,
            get_account_with_inputs,
            invalid_params,
            transaction_builder,
            wait_for_result,
        },
        HandlerContext,
    },
    DEFAULT_FEE,
    NUM_PRESHARDS,
};

const LOG_TARGET: &str = "tari::dan::walletd::handlers::validator";

pub async fn handle_get_validator_fees(
    context: &HandlerContext,
    token: Option<String>,
    req: GetValidatorFeesRequest,
) -> Result<GetValidatorFeesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    let claim_key = match req.account_or_key {
        AccountOrKeyIndex::Account(acc) => {
            let account = get_account_or_default(acc, &sdk.accounts_api())?;
            sdk.key_manager_api()
                .derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?
        },
        AccountOrKeyIndex::KeyIndex(index) => sdk
            .key_manager_api()
            .derive_key(key_manager::TRANSACTION_BRANCH, index)?,
    };
    let claim_public_key = PublicKey::from_secret_key(&claim_key.key);

    let shards = req
        .shard_group
        .map(|sg| Either::Left(sg.shard_iter()))
        .unwrap_or_else(|| Either::Right(NUM_PRESHARDS.all_shards_iter()));

    let addresses = shards.into_iter().map(|shard| {
        (
            shard,
            derive_fee_pool_address(
                byte_utils::copy_fixed(claim_public_key.as_bytes()),
                NUM_PRESHARDS,
                shard,
            ),
        )
    });

    let mut fees = HashMap::new();

    for (shard, address) in addresses {
        let Some(result) = context
            .wallet_sdk()
            .substate_api()
            .scan_for_substate(&SubstateId::from(address), None)
            .await
            .optional()?
        else {
            continue;
        };

        let Some(amount) = result.substate.as_validator_fee_pool().map(|p| p.amount) else {
            warn!(target: LOG_TARGET, "Incorrect substate type found at address {}", address);
            continue;
        };

        fees.insert(shard, FeePoolDetails { amount, address });
    }

    Ok(GetValidatorFeesResponse { fees })
}

#[allow(clippy::too_many_lines)]
pub async fn handle_claim_validator_fees(
    context: &HandlerContext,
    token: Option<String>,
    req: ClaimValidatorFeesRequest,
) -> Result<ClaimValidatorFeesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    sdk.jwt_api().check_auth(token, &[JrpcPermission::Admin])?;

    if req.shards.is_empty() {
        return Err(invalid_params("shards", Some("At least one shard must be specified")));
    }

    let (account, inputs) = get_account_with_inputs(req.account, &sdk)?;
    let account_address = account.address.as_component_address().unwrap();
    let account_secret_key = sdk
        .key_manager_api()
        .derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?;
    let account_public_key = PublicKey::from_secret_key(&account_secret_key.key);

    let (claim_public_key, claim_secret) = match req.claim_key_index {
        Some(index) => {
            let claim_key = sdk
                .key_manager_api()
                .derive_key(key_manager::TRANSACTION_BRANCH, index)?;
            (PublicKey::from_secret_key(&claim_key.key), Some(claim_key))
        },
        None => (PublicKey::from_secret_key(&account_secret_key.key), None),
    };

    let fee_pool_addresses = req.shards.into_iter().map(|shard| {
        derive_fee_pool_address(
            byte_utils::copy_fixed(claim_public_key.as_bytes()),
            NUM_PRESHARDS,
            shard,
        )
    });

    // build the transaction
    let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);

    let transaction = transaction_builder(context)
        .with_fee_instructions_builder(|builder| {
            let mut bucket_names = vec![];
            fee_pool_addresses
                .clone()
                .enumerate()
                .fold(builder, |builder, (i, address)| {
                    bucket_names.push(format!("b{}", i));
                    builder
                        .claim_validator_fees(address)
                        .put_last_instruction_output_on_workspace(bucket_names.last().unwrap())
                })
                .then(|builder| {
                    // TODO: improve this - suggest: the workspace implicitly collect all returned resources (buckets)
                    // and we should create buckets by taking from the workspace. Then we could collect all buckets and
                    // deposit once. Greatly reducing gas for this (and a lot of other) transactions.
                    bucket_names.into_iter().fold(builder, |builder, bucket| {
                        builder.call_method(account_address, "deposit", args![Workspace(bucket)])
                    })
                })
                .call_method(account_address, "pay_fee", args![max_fee])
        })
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .with_inputs(fee_pool_addresses.map(SubstateRequirement::unversioned))
        .then(|builder| {
            if let Some(secret) = claim_secret {
                // If the claim key is different from the account secret, we need to sign with both
                builder
                    .with_authorized_seal_signer()
                    .add_signature(&account_public_key, &secret.key)
            } else {
                builder
            }
        })
        .build_and_seal(&account_secret_key.key);

    // send the transaction
    if req.dry_run {
        let transaction = sdk
            .transaction_api()
            .submit_dry_run_transaction(transaction, vec![])
            .await?;
        return Ok(ClaimValidatorFeesResponse {
            transaction_id: *transaction.transaction.id(),
            fee: transaction
                .finalize
                .as_ref()
                .map(|f| f.fee_receipt.total_fees_paid)
                .unwrap_or_default(),
            result: transaction
                .finalize
                .ok_or_else(|| anyhow!("No finalize result for dry run transaction"))?,
        });
    }

    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction(transaction, vec![])
        .await?;

    let finalized = wait_for_result(&mut events, tx_id).await?;

    if let Some(reject) = finalized.finalize.reject() {
        return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
    }
    if let Some(reason) = finalized.finalize.full_reject() {
        return Err(anyhow::anyhow!(
            "Fee transaction succeeded (fees charged) however the transaction failed: {reason}",
        ));
    }
    info!(
        target: LOG_TARGET,
        "✅ Claim fee transaction {} finalized. Fee: {}",
        finalized.transaction_id,
        finalized.final_fee
    );

    Ok(ClaimValidatorFeesResponse {
        transaction_id: tx_id,
        fee: finalized.final_fee,
        result: finalized.finalize,
    })
}
