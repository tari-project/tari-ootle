// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::fs;

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use indexmap::IndexMap;
use log::info;
use tari_ootle_wallet_crypto::{AlwaysMissLookupTable, IoReaderValueLookup};
use tari_ootle_wallet_sdk::apis::key_manager::KeyBranch;
use tari_template_lib::models::UtxoAddress;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        StealthUtxosDecryptValueRequest,
        StealthUtxosDecryptValueResponse,
        StealthUtxosListRequest,
        StealthUtxosListResponse,
        UtxoInfo,
    },
};
use tokio::{task::block_in_place, time::Instant};

use crate::handlers::{helpers::invalid_params, HandlerContext};

const LOG_TARGET: &str = "tari::walletd::handlers::stealth_utxos";

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosListRequest,
) -> Result<StealthUtxosListResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::AccountList(None)])?;

    let utxos = context.wallet_sdk().stealth_outputs_api().utxos_get_many(
        &req.resource_address,
        req.account_address.as_ref(),
        req.filter_by_status,
    )?;

    Ok(StealthUtxosListResponse {
        utxos: utxos
            .into_iter()
            .map(|o| UtxoInfo {
                address: o.to_utxo_address(),
                value: o.value,
                status: o.status,
                memo: o.memo,
                is_burnt: o.is_burnt,
                is_frozen: o.is_frozen,
                is_on_chain: o.is_on_chain,
            })
            .collect(),
    })
}

pub async fn handle_decrypt_value(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosDecryptValueRequest,
) -> Result<StealthUtxosDecryptValueResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;

    if req.ids.len() > 10 {
        return Err(invalid_params(
            "ids",
            Some("Cannot request more than 10 UTXOs at a time"),
        ));
    }

    let utxo_ids = req
        .ids
        .into_iter()
        .map(|id| UtxoAddress::new(req.resource_address, id))
        .map(Into::into)
        .collect::<Vec<_>>();

    let substates = sdk.substate_api().get_substates_from_network(utxo_ids).await?;

    // Get view secret key
    let view_key = sdk
        .key_manager_api()
        .derive_key(KeyBranch::ElgamalEncryptionViewKey, req.view_key_id)?;

    let value_range = req.minimum_expected_value.unwrap_or(0)..=req.maximum_expected_value.unwrap_or(10_000_000_000);

    // NOTE: we iterate in a random order (HashMap) but collect into a deterministic order (IndexMap) so that the
    // results are always in the same order for the same input
    let proofs = substates
        .iter()
        .filter_map(|(id, s)| {
            let id = id.as_utxo_address().map(|a| a.into_contents().id)?;
            let output = s
                .substate_value()
                .as_utxo()
                .and_then(|u| u.output())
                .and_then(|o| o.output.viewable_balance.as_ref())?;
            Some((id, output))
        })
        .collect::<IndexMap<_, _>>();

    let timer = Instant::now();
    let balances = match context.config().value_lookup_table_file.as_ref() {
        Some(file) => {
            let mut file = fs::File::open(file)
                .map_err(|e| anyhow!("Unable to load value lookup file '{}': {e}", file.display()))?;
            let mut lookup = IoReaderValueLookup::load(&mut file)?;

            block_in_place(|| {
                sdk.viewable_balance_api().try_brute_force_commitment_balances(
                    &view_key.key,
                    proofs.values().copied(), // Copying the reference, not the ElgamalVerifiableBalanceBytes
                    value_range,
                    &mut lookup,
                )
            })?
        },
        None => block_in_place(|| {
            sdk.viewable_balance_api().try_brute_force_commitment_balances(
                &view_key.key,
                proofs.values().copied(),
                value_range,
                &mut AlwaysMissLookupTable,
            )
        })?,
    };

    info!(target: LOG_TARGET, "Brute force balance lookup took {:.2?}", timer.elapsed());

    Ok(StealthUtxosDecryptValueResponse {
        balances: proofs.into_keys().zip(balances).collect(),
    })
}
