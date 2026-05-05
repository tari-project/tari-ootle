//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fs, time::Duration};

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
use log::*;
use ootle_byte_type::ToByteType;
use serde_json::json;
use tari_crypto::{commitment::HomomorphicCommitmentFactory, keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::crypto::{ValueLookupTable, get_commitment_factory};
use tari_ootle_common_types::{displayable::Displayable, optional::Optional};
use tari_ootle_wallet_crypto::{GenerateValueLookup, MMapValueLookup, OutputWitness};
use tari_ootle_wallet_sdk::models::{ConfidentialOutputModel, KeyBranch, OutputStatus};
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        ConfidentialCreateOutputProofRequest,
        ConfidentialCreateOutputProofResponse,
        ConfidentialViewVaultBalanceRequest,
        ConfidentialViewVaultBalanceResponse,
        ProofsCancelRequest,
        ProofsCancelResponse,
        ProofsFinalizeRequest,
        ProofsFinalizeResponse,
        ProofsGenerateRequest,
        ProofsGenerateResponse,
    },
};
use tari_template_lib_types::Amount;
use tokio::{task::block_in_place, time::Instant};

use crate::handlers::{
    HandlerContext,
    helpers::{get_account_or_default, invalid_params, invalid_request},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::json_rpc::confidential";

#[allow(clippy::too_many_lines)]
pub async fn handle_create_transfer_proof(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ProofsGenerateRequest,
) -> Result<ProofsGenerateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    if req.reveal_amount.is_negative() {
        return Err(invalid_request(format!(
            "Amount to send must be positive. Revealed amount was {}",
            req.reveal_amount
        )));
    }

    let account = get_account_or_default(req.account.as_ref(), &sdk.accounts_api())?;
    let account_owner_key_id = account
        .owner_key_id()
        .ok_or_else(|| invalid_request("Account does not have an owner key"))?;
    let vault = sdk
        .accounts_api()
        .get_vault_by_resource(account.component_address(), &req.resource_address)?;
    let lock = sdk.locks_api().create_lock_with_timeout(Duration::from_secs(5 * 60))?;

    let amount_to_transfer = req.confidential_amount.checked_add(req.reveal_amount).ok_or_else(|| {
        invalid_request(format!(
            "Amount to send must be greater than or equal to the amount to reveal. Amount = {}, Revealed = {}",
            req.confidential_amount, req.reveal_amount
        ))
    })?;
    // Lock inputs we're going to spend
    let (inputs, total_input_value) =
        sdk.confidential_outputs_api()
            .lock_outputs_by_amount(lock.id(), &vault.id, amount_to_transfer)?;

    info!(
        target: LOG_TARGET,
        "Locked {} inputs for proof {} worth {} µT",
        inputs.len(),
        lock.id(),
        total_input_value
    );

    // TODO: Any errors from here need to unlock the outputs, ideally just roll back (refactor required but doable).

    // TODO: Wrap up key/encrypted data handling in the wallet SDK
    let account_key = sdk.key_manager_api().get_key(account_owner_key_id)?;
    let output_mask = sdk.key_manager_api().next_key(KeyBranch::ConfidentialMask)?;
    let (_, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());

    let confidential_amount = req.confidential_amount.to_u64_checked().ok_or_else(|| {
        invalid_request(format!(
            "Confidential amount exceeds the maximum value supported in a single UTXO. Amount: {}",
            req.confidential_amount
        ))
    })?;

    let encrypted_data = sdk.confidential_crypto_api().encrypt_value_and_mask(
        confidential_amount,
        &output_mask.key,
        &public_nonce,
        &account_key.secret,
        req.memo.as_ref(),
    )?;

    let resource = sdk.substate_api().fetch_resource(req.resource_address).await?;
    let resource_view_key = resource.to_view_key_public_key().map_err(|_| {
        JsonRpcError::new(
            JsonRpcErrorReason::InvalidRequest,
            "Invalid resource address".to_string(),
            json!({}),
        )
    })?;

    let change_amount = total_input_value.checked_sub(req.confidential_amount).ok_or_else(|| {
        invalid_request(format!(
            "Insufficient funds to send {}. Total input value = {}",
            req.confidential_amount, total_input_value
        ))
    })?;
    let change_amount_u64 = change_amount.to_u64_checked().ok_or_else(|| {
        invalid_request(format!(
            "Change value exceeds the maximum value supported in a single UTXO. Change: {}. Total input value = {}",
            change_amount, total_input_value
        ))
    })?;

    let output_statement = OutputWitness {
        amount: confidential_amount,
        mask: output_mask.key,
        sender_public_nonce: public_nonce,
        minimum_value_promise: 0,
        encrypted_data,
        resource_view_key: resource_view_key.clone(),
    };

    let maybe_change_statement = if change_amount_u64 > 0 {
        let change_mask = sdk.key_manager_api().next_key(KeyBranch::ConfidentialMask)?;
        let (_, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());

        let encrypted_data = sdk.confidential_crypto_api().encrypt_value_and_mask(
            change_amount_u64,
            &change_mask.key,
            &public_nonce,
            &change_mask.key,
            None,
        )?;

        sdk.confidential_outputs_api().add_output(ConfidentialOutputModel {
            account_address: *account.component_address(),
            vault_id: vault.id,
            commitment: get_commitment_factory()
                .commit_value(&change_mask.key, change_amount_u64)
                .to_byte_type(),
            value: change_amount,
            sender_public_nonce: Some(public_nonce.to_byte_type()),
            view_only_key_id: account.view_only_key_id(),
            owner_key_id: account.owner_key_id(),
            encrypted_data: encrypted_data.clone(),
            memo: None,
            public_asset_tag: None,
            status: OutputStatus::LockedUnconfirmed,
            lock_id: Some(lock.id()),
        })?;

        Some(OutputWitness {
            amount: change_amount_u64,
            mask: change_mask.key,
            sender_public_nonce: public_nonce,
            encrypted_data,
            minimum_value_promise: 0,
            resource_view_key,
        })
    } else {
        None
    };

    let inputs = sdk.confidential_outputs_api().resolve_output_masks(inputs)?;

    let proof = sdk.confidential_crypto_api().generate_withdraw_proof(
        &inputs,
        // TODO: support for using revealed funds as input for proof generation
        Amount::zero(),
        Some(&output_statement).filter(|o| o.amount > 0),
        req.reveal_amount,
        maybe_change_statement.as_ref(),
        Amount::zero(),
    )?;

    let lock_id = lock.keep_locked();

    Ok(ProofsGenerateResponse {
        proof_id: lock_id,
        proof,
    })
}

pub async fn handle_finalize_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ProofsFinalizeRequest,
) -> Result<ProofsFinalizeResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let transaction = sdk
        .transaction_api()
        .get(req.transaction_id)
        .optional()?
        .ok_or_else(|| {
            invalid_params(
                "transaction_id",
                Some("No such transaction in wallet to finalize proof for"),
            )
        })?;
    let lock_id = sdk
        .locks_api()
        .get_lock_by_transaction_id(req.transaction_id)
        .optional()?;
    if lock_id != Some(req.lock_id) {
        return Err(invalid_params(
            "lock_id",
            Some("Lock not associated with this transaction"),
        ));
    }

    match transaction.finalized_diff() {
        Some(diff) => {
            info!(
                target: LOG_TARGET,
                "Finalizing locked proof {} for transaction {}",
                req.lock_id,
                req.transaction_id
            );
            sdk.locks_api().finalize_lock(req.lock_id, diff)?;
        },
        None => {
            return Err(invalid_params(
                "transaction_id",
                Some(format!(
                    "Transaction is not finalized (status = {}, reason = {})",
                    transaction.status,
                    transaction.failure_reason_as_string().display()
                )),
            ));
        },
    }

    Ok(ProofsFinalizeResponse {})
}

pub async fn handle_cancel_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ProofsCancelRequest,
) -> Result<ProofsCancelResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    sdk.locks_api().release_lock(req.proof_id)?;
    Ok(ProofsCancelResponse {})
}

pub async fn handle_create_output_proof(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ConfidentialCreateOutputProofRequest,
) -> Result<ConfidentialCreateOutputProofResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let output_mask = sdk.key_manager_api().next_key(KeyBranch::ConfidentialMask)?;
    let (_, public_nonce) = RistrettoPublicKey::random_keypair(&mut rand::rng());
    let encrypted_data = sdk.confidential_crypto_api().encrypt_value_and_mask(
        req.amount,
        &output_mask.key,
        &public_nonce,
        &output_mask.key,
        // TODO: Support memos
        None,
    )?;

    let statement = OutputWitness {
        amount: req.amount,
        mask: output_mask.key,
        sender_public_nonce: public_nonce,
        minimum_value_promise: 0,
        encrypted_data,
        // TODO: the request must include the resource address so that we can fetch the view key
        resource_view_key: None,
    };
    let proof = sdk
        .confidential_crypto_api()
        .generate_output_proof(&statement, Amount::zero())?;
    Ok(ConfidentialCreateOutputProofResponse { proof })
}

pub async fn handle_view_vault_balance(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ConfidentialViewVaultBalanceRequest,
) -> Result<ConfidentialViewVaultBalanceResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    let substate = sdk
        .substate_api()
        .fetch_substate_from_network(&req.vault_id.into(), None)
        .await?;
    let vault = substate
        .substate
        .as_vault()
        .ok_or_else(|| anyhow::anyhow!("Indexer returned a non-vault substate when scanning for a vault address"))?;

    let commitments = vault
        .get_confidential_commitments()
        .ok_or_else(|| invalid_params("vault_id", Some("Vault does not contain a confidential resource")))?;

    // Get view secret key
    let view_key = sdk.key_manager_api().get_elgamal_encrypted_view_key(req.view_key_id)?;

    let value_range = req.minimum_expected_value.unwrap_or(0)..=req.maximum_expected_value.unwrap_or(10_000_000_000);

    let timer = Instant::now();
    let balances = match context.config().value_lookup_table_file.as_ref() {
        Some(file) => {
            let file = fs::File::open(file)
                .map_err(|e| anyhow!("Unable to load value lookup file '{}': {e}", file.display()))?;
            let mut is_logged = false;
            // SAFETY: We assume the file will not be modified while mapped. Although not enforced (e.g. locks,
            // permissions and other platform specific mechanisms), this is a reasonable assumption for most scenarios.
            let mut lookup = unsafe { MMapValueLookup::load(&file) }?.with_fallback(move |v| {
                if !is_logged {
                    is_logged = true;
                    warn!("Using value lookup fallback. This will likely result in very slow lookups.");
                }
                GenerateValueLookup.lookup(v)
            });

            block_in_place(|| {
                sdk.viewable_balance_api().try_brute_force_commitment_balances(
                    &view_key.key,
                    commitments.values().filter_map(|o| o.viewable_balance.as_ref()),
                    value_range,
                    &mut lookup,
                )
            })?
        },
        None => {
            warn!(
                target: LOG_TARGET,
                "No value lookup table configured. This will likely result in very slow lookups."
            );
            block_in_place(|| {
                sdk.viewable_balance_api().try_brute_force_commitment_balances(
                    &view_key.key,
                    commitments.values().filter_map(|o| o.viewable_balance.as_ref()),
                    value_range,
                    &mut GenerateValueLookup,
                )
            })?
        },
    };

    info!(target: LOG_TARGET, "Brute force balance lookup took {:.2?}", timer.elapsed());

    Ok(ConfidentialViewVaultBalanceResponse {
        balances: commitments
            .iter()
            .filter_map(|(id, o)| o.viewable_balance.as_ref().map(|_| *id))
            .zip(balances)
            .collect(),
    })
}
