// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::fs;

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use indexmap::IndexMap;
use log::{info, warn};
use tari_engine_types::crypto::ValueLookupTable;
use tari_ootle_wallet_crypto::{GenerateValueLookup, MMapValueLookup};
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        StealthUtxosDecryptValueRequest,
        StealthUtxosDecryptValueResponse,
        StealthUtxosListRequest,
        StealthUtxosListResponse,
        UtxoInfo,
    },
};
use tari_template_lib_types::UtxoAddress;
use tokio::{
    task::{AbortHandle, spawn_blocking},
    time::Instant,
};

use crate::handlers::{HandlerContext, helpers::invalid_params};

const LOG_TARGET: &str = "tari::ootle::walletd::handlers::stealth_utxos";

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
                value: o.value.into(),
                status: o.status,
                memo: o.memo,
                spend_condition: o.spend_condition,
                is_burnt: o.is_burnt,
                is_frozen: o.is_frozen,
                is_on_chain: o.is_on_chain,
            })
            .collect(),
    })
}

#[allow(clippy::too_many_lines)]
pub async fn handle_decrypt_value(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosDecryptValueRequest,
) -> Result<StealthUtxosDecryptValueResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.check_auth(token, &[JrpcPermission::Admin])?;
    if req.ids.is_empty() {
        return Err(invalid_params("ids", Some("At least one UTXO ID must be provided")));
    }

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
    let view_key = sdk.key_manager_api().get_elgamal_encrypted_view_key(req.view_key_id)?;

    let value_range = req.minimum_expected_value.unwrap_or(0)..=req.maximum_expected_value;

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
    let elgamal_proofs = proofs.values().copied().cloned().collect::<Vec<_>>();
    let sdk = sdk.clone();
    let handle = match context.config().value_lookup_table_file.clone() {
        Some(path) => spawn_blocking(move || {
            let file = fs::File::open(&path)
                .map_err(|e| anyhow!("Unable to load value lookup file '{}': {e}", path.display()))?;
            // SAFETY: We assume the file will not be modified while mapped. Although not enforced (e.g. locks,
            // permissions and other platform specific mechanisms), this is a reasonable assumption for most scenarios.
            let lookup = unsafe { MMapValueLookup::load(&file) }?;

            info!(
                target: LOG_TARGET,
                "Using value lookup table from file '{}' ({}-{}) for brute force balance lookup",
                path.display(),
                lookup.range().start(),
                lookup.range().end()
            );

            let start = value_range.start();
            let end = value_range.end();
            if start < lookup.range().start() || end > lookup.range().end() {
                warn!(
                    target: LOG_TARGET,
                    "The requested value range ({}-{}) is outside the loaded value lookup table range ({}-{}). \
                     This query may take excessive amount of time.",
                    start,
                    end,
                    lookup.range().start(),
                    lookup.range().end()
                );
            }

            let mut is_logged = false;
            let mut lookup = lookup.with_fallback(move |v| {
                if !is_logged {
                    is_logged = true;
                    warn!(target: LOG_TARGET, "Using value lookup fallback. This will likely result in very slow lookups.");
                }
                GenerateValueLookup.lookup(v)
            });

            let balance = sdk.viewable_balance_api().try_brute_force_commitment_balances(
                &view_key.key,
                elgamal_proofs.iter(),
                value_range,
                &mut lookup,
            )?;

            anyhow::Ok(balance)
        }),
        None => {
            warn!(
                target: LOG_TARGET,
                "No value lookup table file configured. Using a generated value lookup fallback. \
                 Brute-force may still be slow for very high-value outputs."
            );
            spawn_blocking(move || {
                let balances = sdk.viewable_balance_api().try_brute_force_commitment_balances(
                    &view_key.key,
                    elgamal_proofs.iter(),
                    value_range,
                    &mut GenerateValueLookup,
                )?;
                anyhow::Ok(balances)
            })
        },
    };
    struct AbortOnDropGuard {
        handle: AbortHandle,
    }

    impl Drop for AbortOnDropGuard {
        fn drop(&mut self) {
            if !self.handle.is_finished() {
                info!(target: LOG_TARGET, "Aborting brute force balance lookup task");
                self.handle.abort();
            }
        }
    }

    // If this request is abandoned, abort the blocking task
    let _drop_guard = AbortOnDropGuard {
        handle: handle.abort_handle(),
    };
    let balances = handle.await??;

    info!(target: LOG_TARGET, "Brute force balance lookup took {:.2?}", timer.elapsed());

    Ok(StealthUtxosDecryptValueResponse {
        values: proofs.into_keys().zip(balances).collect(),
    })
}
