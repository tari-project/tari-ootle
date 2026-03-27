//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use either::Either;
use log::*;
use ootle_byte_type::ToByteType;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateAddress, SubstateRequirement, derive_fee_pool_address};
use tari_ootle_transaction::args;
use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId};
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        AccountOrKeyId,
        ClaimValidatorFeesRequest,
        ClaimValidatorFeesResponse,
        FeePoolDetails,
        GetValidatorFeesRequest,
        GetValidatorFeesResponse,
    },
};

use crate::{
    DEFAULT_FEE,
    NUM_PRESHARDS,
    handlers::{
        HandlerContext,
        helpers::{get_account_or_default, get_account_with_inputs, invalid_params, wait_for_result},
    },
};

const LOG_TARGET: &str = "tari::ootle::walletd::handlers::validator";

pub async fn handle_get_validator_fees(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: GetValidatorFeesRequest,
) -> Result<GetValidatorFeesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let claim_key = match req.account_or_key {
        AccountOrKeyId::Account(acc) => {
            let account = get_account_or_default(acc.as_ref(), &sdk.accounts_api())?;
            let account_key_id = account.owner_key_id().ok_or_else(|| {
                anyhow!("The specified account does not have an associated owner key to derive the claim key from")
            })?;
            sdk.key_manager_api().get_public_key(account_key_id)?
        },
        AccountOrKeyId::KeyId(key_id) => sdk.key_manager_api().get_public_key(key_id)?,
    };
    let claim_public_key = claim_key.public_key().to_byte_type();

    let shards = req
        .shard_group
        .map(|sg| Either::Left(sg.shard_iter()))
        .unwrap_or_else(|| Either::Right(NUM_PRESHARDS.all_shards_iter()));

    let ids = shards
        .into_iter()
        .map(|shard| derive_fee_pool_address(&claim_public_key, NUM_PRESHARDS, shard))
        .map(SubstateId::from)
        .collect::<Vec<_>>();

    let mut fees = HashMap::with_capacity(ids.len());
    const CHUNK_SIZE: usize = 20;
    for id_chunk in ids.chunks(CHUNK_SIZE) {
        let substates = context
            .wallet_sdk()
            .substate_api()
            .get_substates_from_network(id_chunk.to_vec())
            .await?;

        info!(target: LOG_TARGET, "🔍️ Found {}/{} fee pool substates for claim key {}", substates.len(), CHUNK_SIZE, claim_public_key);

        for (substate_id, substate) in substates {
            let Some(address) = substate_id.as_validator_fee_pool_address() else {
                warn!(target: LOG_TARGET, "Incorrect substate ID found: {}", substate_id);
                continue;
            };

            let Some(amount) = substate.substate_value().as_validator_fee_pool().map(|p| p.amount()) else {
                warn!(target: LOG_TARGET, "Incorrect substate type found at address {}", substate_id);
                continue;
            };

            if amount > 0 {
                let shard = SubstateAddress::from_substate_id(&substate_id, substate.version()).to_shard(NUM_PRESHARDS);
                fees.insert(shard, FeePoolDetails { amount, address });
            }
        }
    }

    Ok(GetValidatorFeesResponse { fees })
}

#[allow(clippy::too_many_lines)]
pub async fn handle_claim_validator_fees(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ClaimValidatorFeesRequest,
) -> Result<ClaimValidatorFeesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    if req.shards.is_empty() {
        return Err(invalid_params("shards", Some("At least one shard must be specified")));
    }

    let (account, inputs) = get_account_with_inputs(req.account.as_ref(), &sdk)?;
    let account_key_id = account.owner_key_id().ok_or_else(|| {
        anyhow!("The specified account does not have an associated owner key to derive the claim key from")
    })?;
    let account_component_address = *account.component_address();

    let claim_public_key = match req.claim_key_index {
        Some(index) => sdk
            .key_manager_api()
            .get_public_key(KeyId::derived(KeyBranch::Account, index))?
            .public_key
            .to_byte_type(),
        None => *account.address.account_public_key(),
    };

    let fee_pool_addresses = req
        .shards
        .into_iter()
        .map(|shard| derive_fee_pool_address(&claim_public_key, NUM_PRESHARDS, shard));

    // build the transaction
    let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);

    let unsigned_transaction = context
        .transaction_builder()
        .with_dry_run(req.dry_run)
        .with_fee_instructions_builder(|builder| {
            builder
                .create_account(*account.address.account_public_key())
                .then(|builder| {
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
                            // TODO: improve this - suggest: the workspace implicitly collect all returned resources
                            // (buckets) and we should create buckets by taking from the
                            // workspace. Then we could collect all buckets and
                            // deposit once. Greatly reducing gas for this (and a lot of other) transactions.
                            bucket_names.into_iter().fold(builder, |builder, bucket| {
                                builder.call_method(account_component_address, "deposit", args![Workspace(bucket)])
                            })
                        })
                })
                .call_method(account_component_address, "pay_fee", args![max_fee])
        })
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .with_inputs(fee_pool_addresses.map(SubstateRequirement::unversioned))
        .map(|builder| {
            if let Some(index) = req.claim_key_index {
                if claim_public_key == *account.address.account_public_key() {
                    Ok(builder.finish())
                } else {
                    // If the claim key is different from the account secret, we need to sign with both
                    sdk.signer_api()
                        .with_context(account.address.account_public_key())
                        .sign(KeyId::derived(KeyBranch::Account, index), builder.finish())
                }
            } else {
                Ok(builder.finish())
            }
        })?;

    let transaction = sdk.signer_api().sign(account_key_id, unsigned_transaction)?;

    // send the transaction
    if req.dry_run {
        let transaction = sdk.transaction_api().submit_dry_run_transaction(transaction).await?;
        return Ok(ClaimValidatorFeesResponse {
            transaction_id: transaction.id,
            fee: transaction
                .finalize
                .as_ref()
                .map(|f| f.fee_receipt.total_fees_paid())
                .unwrap_or_default(),
            result: transaction
                .finalize
                .ok_or_else(|| anyhow!("No finalize result for dry run transaction"))?,
        });
    }

    let mut events = context.notifier().subscribe();
    let tx_id = context.transaction_service().submit_transaction(transaction).await?;

    let finalized = wait_for_result(&mut events, tx_id).await?;

    if let Some(reject) = finalized.finalize.fee_reject() {
        return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
    }
    if let Some(reason) = finalized.finalize.any_reject() {
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
