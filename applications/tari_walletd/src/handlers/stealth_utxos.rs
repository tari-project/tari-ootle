// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use indexmap::IndexMap;
use log::info;
use tari_ootle_address::{Network, OotleAddress, PayRef};
use tari_ootle_wallet_crypto::memo::Memo;
use tari_ootle_walletd_client::{
    permissions::{Crud, Permission},
    types::{
        StealthUtxosDecryptValueRequest,
        StealthUtxosDecryptValueResponse,
        StealthUtxosGetValueLookupInfoRequest,
        StealthUtxosGetValueLookupInfoResponse,
        StealthUtxosListRequest,
        StealthUtxosListResponse,
        UtxoInfo,
    },
};
use tari_template_lib_types::{UtxoAddress, crypto::RistrettoPublicKeyBytes};
use tokio::{task::spawn_blocking, time::Instant};

use crate::handlers::{
    HandlerContext,
    helpers::invalid_params,
    value_lookup::{AbortQueuedOnDropGuard, ValueRangeRequest},
};

const LOG_TARGET: &str = "tari::ootle::walletd::handlers::stealth_utxos";

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosListRequest,
) -> Result<StealthUtxosListResponse, anyhow::Error> {
    context.authorize(token, &[Permission::StealthUtxos(Crud::Read, req.account_address)])?;

    let network = context.wallet_sdk().network();

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
                sender_address: resolve_sender_address(o.memo.as_ref(), network),
                memo: o.memo,
                auth: o.auth,
                is_burnt: o.is_burnt,
                is_frozen: o.is_frozen,
                is_on_chain: o.is_on_chain,
            })
            .collect(),
    })
}

/// Reconstructs the sender's [`OotleAddress`] from a `SenderAddress` memo using the wallet's network, including
/// any attached pay reference. Returns `None` for any other memo variant or if the embedded keys are malformed.
fn resolve_sender_address(memo: Option<&Memo>, network: Network) -> Option<OotleAddress> {
    let (account_key, view_key, pay_ref) = memo?.as_sender_address()?;
    let account_key = RistrettoPublicKeyBytes::from_bytes(account_key).ok()?;
    let view_key = RistrettoPublicKeyBytes::from_bytes(view_key).ok()?;
    let mut address = OotleAddress::new(network, view_key, account_key);
    if let Some(pay_ref) = PayRef::from_bytes(pay_ref) {
        address = address.with_pay_ref(pay_ref);
    }
    Some(address)
}

#[allow(clippy::too_many_lines)]
pub async fn handle_decrypt_value(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthUtxosDecryptValueRequest,
) -> Result<StealthUtxosDecryptValueResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &[Permission::StealthUtxos(Crud::Read, None)])?;
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
    let view_key = sdk.key_manager_api().get_key(req.view_key_id)?;

    let value_range = ValueRangeRequest::resolve(req.minimum_expected_value, req.maximum_expected_value);

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
    let lookup_file = context.config().value_lookup_table_file.clone();
    let handle = spawn_blocking(move || {
        crate::handlers::value_lookup::brute_force_viewable_balances(
            &sdk.viewable_balance_api(),
            lookup_file.as_deref(),
            view_key.secret(),
            &elgamal_proofs,
            value_range,
        )
    });
    let _drop_guard = AbortQueuedOnDropGuard::new(handle.abort_handle());
    let recovery = handle.await??;

    info!(target: LOG_TARGET, "Brute force balance lookup took {:.2?}", timer.elapsed());

    Ok(StealthUtxosDecryptValueResponse {
        values: proofs.into_keys().zip(recovery.balances).collect(),
        searched: recovery.searched,
    })
}

pub async fn handle_get_value_lookup_info(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _req: StealthUtxosGetValueLookupInfoRequest,
) -> Result<StealthUtxosGetValueLookupInfoResponse, anyhow::Error> {
    context.authorize(token, &[Permission::StealthUtxos(Crud::Read, None)])?;
    crate::handlers::value_lookup::value_lookup_info(context.config().value_lookup_table_file.as_deref())
}
