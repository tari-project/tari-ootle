//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, iter};

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use either::Either;
use log::*;
use ootle_byte_type::ToByteType;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateAddress, SubstateRequirement, derive_fee_pool_address};
use tari_ootle_transaction::args;
use tari_ootle_wallet_crypto::{OutputWitness, StealthInputWitness, StealthOutputWitness, memo::Memo};
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
use tari_template_lib_types::{
    ValidatorFeePoolAddress,
    constants::{STEALTH_TARI_RESOURCE_ADDRESS, TARI_TOKEN},
    stealth::{SpendCondition, StealthTransferStatement},
};

use crate::{
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

    let fee_pool_addresses: Vec<ValidatorFeePoolAddress> = req
        .shards
        .iter()
        .map(|shard| derive_fee_pool_address(&claim_public_key, NUM_PRESHARDS, *shard))
        .collect();

    // build the transaction
    let max_fee = req.max_fee.max(1);
    let account_public_key = *account.address.account_public_key();

    let builder = context.transaction_builder().with_dry_run(req.dry_run);

    let builder = if req.output_to_revealed {
        builder
            .with_fee_instructions_builder(|builder| {
                builder
                    .create_account(account_public_key)
                    .then(|builder| {
                        let mut bucket_names = vec![];
                        fee_pool_addresses
                            .iter()
                            .copied()
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
    } else {
        let plan = build_self_stealth_plan(&sdk, &account, account_key_id, &fee_pool_addresses, max_fee).await?;
        builder.with_fee_instructions_builder(move |builder| {
            let mut builder = builder;
            for (i, (address, statement)) in plan.statements.into_iter().enumerate() {
                let bucket = format!("b{}", i);
                builder = builder
                    .claim_validator_fees(address)
                    .put_last_instruction_output_on_workspace(bucket.clone())
                    .stealth_transfer_with_input_bucket(TARI_TOKEN, statement, bucket);
                if i == plan.fee_carrier_idx {
                    builder = builder.put_last_instruction_output_on_workspace("fee");
                }
            }
            builder.pay_fee_from_bucket("fee")
        })
    };

    let unsigned_transaction = builder
        .with_inputs(fee_pool_addresses.iter().copied().map(SubstateRequirement::unversioned))
        .map(|builder| {
            if let Some(index) = req.claim_key_index {
                if claim_public_key == account_public_key {
                    Ok(builder.finish())
                } else {
                    // If the claim key is different from the account secret, we need to sign with both
                    sdk.signer_api()
                        .with_context(&account_public_key)
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
            // Dry-run forces a minimal max_fee so the call doesn't require funded vaults, which clamps
            // `total_fees_paid` to that placeholder. Report the uncapped estimate instead.
            fee: transaction
                .finalize
                .as_ref()
                .map(|f| f.fee_receipt.required_fees())
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

struct StealthClaimPlan {
    statements: Vec<(ValidatorFeePoolAddress, StealthTransferStatement)>,
    /// Index of the pool whose stealth_transfer carves out `max_fee` as a revealed-output bucket to pay the network
    /// fee. All other pools carve out 0.
    fee_carrier_idx: usize,
}

/// Fetches each fee pool's current amount and builds a per-shard [`StealthTransferStatement`] that converts the
/// claimed revealed amount into a stealth UTXO addressed to the account's own owner key. The pool with the largest
/// amount additionally carves `max_fee` as a revealed-output bucket which the caller pays the network fee from — so
/// no funds need to come from the user's account.
async fn build_self_stealth_plan(
    sdk: &crate::WalletSdk,
    account: &tari_ootle_wallet_sdk::models::AccountWithAddress,
    account_owner_key_id: tari_ootle_wallet_sdk::models::KeyId,
    fee_pool_addresses: &[ValidatorFeePoolAddress],
    max_fee: u64,
) -> Result<StealthClaimPlan, anyhow::Error> {
    let network = sdk.config_api().get_network()?;
    let account_owner = sdk.key_manager_api().get_public_key(account_owner_key_id)?;
    let view_only = sdk.key_manager_api().get_public_key(account.view_only_key_id())?;

    let substate_ids: Vec<SubstateId> = fee_pool_addresses.iter().copied().map(SubstateId::from).collect();
    let mut amounts: HashMap<ValidatorFeePoolAddress, u64> = HashMap::with_capacity(substate_ids.len());
    const CHUNK_SIZE: usize = 20;
    for chunk in substate_ids.chunks(CHUNK_SIZE) {
        let substates = sdk.substate_api().get_substates_from_network(chunk.to_vec()).await?;
        for (id, substate) in substates {
            let Some(addr) = id.as_validator_fee_pool_address() else {
                continue;
            };
            let Some(amount) = substate.substate_value().as_validator_fee_pool().map(|p| p.amount()) else {
                continue;
            };
            amounts.insert(addr, amount);
        }
    }

    let (fee_carrier_idx, fee_carrier_amount) = fee_pool_addresses
        .iter()
        .enumerate()
        .map(|(i, addr)| (i, amounts.get(addr).copied().unwrap_or(0)))
        .max_by_key(|(_, amount)| *amount)
        .ok_or_else(|| invalid_params("shards", Some("no fee pool addresses to claim")))?;

    if fee_carrier_amount < max_fee {
        return Err(invalid_params(
            "max_fee",
            Some(format!(
                "max_fee ({max_fee}) exceeds the largest claimable fee pool amount ({fee_carrier_amount}); reduce \
                 max_fee or include shards with larger balances"
            )),
        ));
    }

    let memo = Memo::new_message("Validator fees claimed to stealth").expect("valid memo");

    let statements = fee_pool_addresses
        .iter()
        .copied()
        .enumerate()
        .map(|(i, address)| {
            let amount = amounts.get(&address).copied().unwrap_or(0);
            if amount == 0 {
                return Err(invalid_params(
                    "shards",
                    Some(format!("Fee pool {address} is empty or could not be fetched")),
                ));
            }

            let revealed_output = if i == fee_carrier_idx { max_fee } else { 0 };
            let stealth_amount = amount - revealed_output;

            let mask = sdk.key_manager_api().next_key(KeyBranch::StealthMask)?;
            let (nonce, output_public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());

            let encrypted_data = sdk.stealth_crypto_api().encrypt_value_and_mask(
                stealth_amount,
                &mask.key,
                view_only.public_key(),
                &nonce,
                Some(&memo),
            )?;

            let tag = sdk.stealth_crypto_api().derive_stealth_output_tag(
                network,
                &nonce,
                view_only.public_key(),
                &STEALTH_TARI_RESOURCE_ADDRESS,
            );

            let stealth_owner_public_key =
                sdk.stealth_crypto_api()
                    .derive_stealth_owner_public_key(network, account_owner.public_key(), &nonce);

            let output_witness = StealthOutputWitness {
                witness: OutputWitness {
                    amount: stealth_amount,
                    mask: mask.key,
                    sender_public_nonce: output_public_nonce,
                    minimum_value_promise: 0,
                    encrypted_data,
                    resource_view_key: None,
                },
                spend_condition: SpendCondition::Signed(stealth_owner_public_key.to_byte_type()),
                tag,
            };

            let statement = sdk.stealth_crypto_api().generate_transfer_statement(
                iter::empty::<StealthInputWitness>(),
                amount,
                iter::once(&output_witness),
                revealed_output,
            )?;

            Ok((address, statement))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(StealthClaimPlan {
        statements,
        fee_carrier_idx,
    })
}
