//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Create / approve / submit for transaction requests (issue #2343).
//!
//! A limited-permission tool creates a request; a principal holding
//! `transaction_requests:approve` authorises it; submit seals it. The three are
//! separately permissioned, so a tool granted only `:create` cannot approve the
//! requests it creates.
//!
//! The transaction is **frozen at creation**: stored verbatim (the caller
//! supplies complete inputs and any out-of-band signatures). The approver views
//! exactly what will be sealed, and submit seals it unchanged.

use std::time::Duration;

use axum_extra::headers::authorization::Bearer;
use log::*;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::{
    models::{
        EffectiveStatus,
        OutputStatus,
        TransactionRequestCreatedEvent,
        TransactionRequestId,
        TransactionRequestModel,
        TransactionRequestStatus,
        WalletEvent,
        WalletLockId,
    },
    storage::{CommittableStore, ReadableWalletStore, WalletStoreReader, WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_walletd_client::{
    permissions::{Permission, TxRequestAction},
    types::{
        TransactionRequestCreateRequest,
        TransactionRequestCreateResponse,
        TransactionRequestDecisionRequest,
        TransactionRequestDecisionResponse,
        TransactionRequestGetRequest,
        TransactionRequestGetResponse,
        TransactionRequestInfo,
        TransactionRequestListRequest,
        TransactionRequestListResponse,
        TransactionRequestSubmitRequest,
        TransactionRequestSubmitResponse,
        TransactionRequestValueSummary,
        TransactionSubmitRequest,
    },
};

use super::context::HandlerContext;
use crate::handlers::{
    helpers::{invalid_params, invalid_request},
    transaction::submit_inner_for_request,
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::transaction_requests";

/// Extra margin on the locks beyond the approval window.
///
/// The lock must outlive the request, never the reverse: a lock released under
/// an Approved request would let submit sign a spend of UTXOs that are
/// spendable again, whereas a lock outliving a dead request merely idles funds
/// until the stale sweep takes it.
const LOCK_GRACE: Duration = Duration::from_secs(5 * 60);

/// Upper bound on a caller-supplied approval window. A request idling input
/// locks for longer than this is not an approval flow, and the bound keeps the
/// `ttl + LOCK_GRACE` and expiry-timestamp arithmetic trivially in range.
const MAX_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub async fn handle_create(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionRequestCreateRequest,
) -> Result<TransactionRequestCreateResponse, anyhow::Error> {
    let auth = context.authorize_with_identity(token, &[Permission::TransactionRequests(TxRequestAction::Create)])?;
    let sdk = context.wallet_sdk();

    req.transaction
        .validate_blob_references()
        .map_err(|e| invalid_params("transaction.blobs", Some(e.to_string())))?;

    // The transaction is stored verbatim: the request path neither detects
    // inputs nor touches any field, so a caller who has already collected
    // signatures over these exact bytes is not invalidated. Callers that need
    // inputs resolved run `transactions.detect_inputs` first.
    let unsigned = req.transaction;

    let ttl = req
        .ttl_secs
        .map(Duration::from_secs)
        .unwrap_or(context.config().transaction_request_ttl);
    if ttl > MAX_TTL {
        return Err(invalid_params(
            "ttl_secs",
            Some(format!("must be at most {} seconds", MAX_TTL.as_secs())),
        ));
    }
    let model = {
        let mut tx = sdk.store().create_write_tx()?;

        // Hold the inputs across the approval window. The transfer-selection
        // handlers lock for five minutes, which is a selection deadline, not a
        // deadline a person can meet.
        for lock_id in &req.lock_ids {
            tx.locks_set_timeout(*lock_id, Some(ttl + LOCK_GRACE))?;
        }

        let model = tx.transaction_request_insert(
            &unsigned,
            req.seal_signer,
            &req.other_signers,
            &req.signatures,
            &req.lock_ids,
            auth.api_key_name.as_deref(),
            ttl,
        )?;
        tx.commit()?;
        model
    };

    info!(
        target: LOG_TARGET,
        "Transaction request {} created by {} ({} lock(s), expires {})",
        model.id,
        auth.api_key_name.as_deref().unwrap_or("a wallet session"),
        req.lock_ids.len(),
        model.expires_at,
    );

    context
        .notifier()
        .notify(WalletEvent::TransactionRequestCreated(TransactionRequestCreatedEvent {
            request_id: model.id,
            requested_by: auth.api_key_name.clone(),
        }));

    Ok(TransactionRequestCreateResponse {
        request_id: model.id,
        expires_at: model.expires_at.assume_utc().unix_timestamp(),
    })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionRequestGetRequest,
) -> Result<TransactionRequestGetResponse, anyhow::Error> {
    context.authorize(token, &[Permission::TransactionRequests(TxRequestAction::Read)])?;

    let model = context
        .wallet_sdk()
        .store()
        .with_read_tx(|tx| tx.transaction_request_get(req.request_id))?;

    Ok(TransactionRequestGetResponse {
        request: to_info(context, model)?,
    })
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionRequestListRequest,
) -> Result<TransactionRequestListResponse, anyhow::Error> {
    context.authorize(token, &[Permission::TransactionRequests(TxRequestAction::Read)])?;

    let models = context
        .wallet_sdk()
        .store()
        .with_read_tx(|tx| tx.transaction_requests_list())?;

    let requests = models
        .into_iter()
        .map(|m| to_info(context, m))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        // Expiry is derived, so it cannot be filtered in SQL.
        .filter(|info| req.status.is_none_or(|s| s == info.status))
        .collect();

    Ok(TransactionRequestListResponse { requests })
}

pub async fn handle_approve(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionRequestDecisionRequest,
) -> Result<TransactionRequestDecisionResponse, anyhow::Error> {
    // Deliberately NOT `authorize_user_only`: approve is grantable to a tool.
    // The control is who holds the scope, not what kind of credential they are.
    context.authorize(token, &[Permission::TransactionRequests(TxRequestAction::Approve)])?;

    let model = transition(
        context,
        req.request_id,
        &[TransactionRequestStatus::Pending],
        TransactionRequestStatus::Approved,
    )?;

    info!(target: LOG_TARGET, "Transaction request {} approved", req.request_id);

    Ok(TransactionRequestDecisionResponse {
        request_id: req.request_id,
        status: model.effective_status_now(),
    })
}

pub async fn handle_reject(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionRequestDecisionRequest,
) -> Result<TransactionRequestDecisionResponse, anyhow::Error> {
    context.authorize(token, &[Permission::TransactionRequests(TxRequestAction::Approve)])?;

    // An approved-but-unsubmitted request is still cancellable: approval is a
    // decision, not a commitment, and without this the only way out is letting
    // the request idle its input locks until expiry. Only an in-flight submit
    // (Submitting) is past the point of refusal.
    let model = transition(
        context,
        req.request_id,
        &[TransactionRequestStatus::Pending, TransactionRequestStatus::Approved],
        TransactionRequestStatus::Rejected,
    )?;

    // Release the inputs immediately rather than idling them until the sweep:
    // a refused request is never going to spend them.
    for lock_id in &model.lock_ids {
        if let Err(e) = context.wallet_sdk().locks_api().release_lock(*lock_id) {
            warn!(
                target: LOG_TARGET,
                "Failed to release lock {lock_id} for rejected request {}: {e}", req.request_id,
            );
        }
    }

    Ok(TransactionRequestDecisionResponse {
        request_id: req.request_id,
        status: model.effective_status_now(),
    })
}

pub async fn handle_submit(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: TransactionRequestSubmitRequest,
) -> Result<TransactionRequestSubmitResponse, anyhow::Error> {
    // Submitting an approved request is not the same authority as
    // `transactions:create` (unrestricted submit): the transaction is frozen at
    // creation, so this confers no ability to alter what gets signed.
    context.authorize(token, &[Permission::TransactionRequests(TxRequestAction::Create)])?;
    let sdk = context.wallet_sdk();

    // Claim the request before any sealing work: sealing uses a random nonce,
    // so two concurrent submitters that both passed a status *check* would
    // produce two distinct transactions spending the same inputs, and the
    // locks would end up linked to whichever sealed last. The conditional
    // Approved -> Submitting UPDATE lets exactly one caller proceed; the rest
    // fail here with the request untouched.
    let model = transition(
        context,
        req.request_id,
        &[TransactionRequestStatus::Approved],
        TransactionRequestStatus::Submitting,
    )?;

    let submit = TransactionSubmitRequest {
        transaction: model.unsigned_transaction,
        seal_signer: model.seal_signer,
        other_signers: model.other_signers,
        signatures: model.signatures,
        // Everything was resolved at creation. Detecting again here would
        // change the transaction the approver saw.
        detect_inputs: false,
        detect_inputs_use_unversioned: true,
        lock_ids: model.lock_ids,
    };

    let response = match submit_inner_for_request(context, submit).await {
        Ok(response) => response,
        Err(e) => {
            // Release the claim so the approval survives a failed submit and
            // another holder can retry. Best-effort: if this also fails the
            // claim expires with the request and the stale sweep reclaims the
            // (still unlinked) locks.
            if let Err(release_err) = sdk.store().with_write_tx(|tx| {
                tx.transaction_request_transition(
                    req.request_id,
                    &[TransactionRequestStatus::Submitting],
                    TransactionRequestStatus::Approved,
                )
            }) {
                warn!(
                    target: LOG_TARGET,
                    "Failed to release submit claim on transaction request {}: {release_err}", req.request_id,
                );
            }
            return Err(e);
        },
    };

    // Record what it became, so the approval is linked to the transaction it
    // authorised rather than dead-ending at "Submitted".
    sdk.store()
        .with_write_tx(|tx| tx.transaction_request_mark_submitted(req.request_id, response.transaction_id))?;

    info!(
        target: LOG_TARGET,
        "Transaction request {} submitted as {}", req.request_id, response.transaction_id,
    );

    Ok(TransactionRequestSubmitResponse {
        transaction_id: response.transaction_id,
    })
}

/// Guarded transition to `to`, refusing an already-expired request.
fn transition(
    context: &HandlerContext,
    request_id: TransactionRequestId,
    from: &[TransactionRequestStatus],
    to: TransactionRequestStatus,
) -> Result<TransactionRequestModel, anyhow::Error> {
    let sdk = context.wallet_sdk();

    let current = sdk.store().with_read_tx(|tx| tx.transaction_request_get(request_id))?;

    // A decision on a request that has already timed out is not a decision.
    if matches!(current.effective_status_now(), EffectiveStatus::Expired) {
        return Err(invalid_request(format!(
            "Transaction request {request_id} expired at {}",
            current.expires_at
        )));
    }

    let model = sdk
        .store()
        .with_write_tx(|tx| tx.transaction_request_transition(request_id, from, to))?;

    Ok(model)
}

/// The wallet-computed view of a request, including what it moves.
fn to_info(context: &HandlerContext, model: TransactionRequestModel) -> Result<TransactionRequestInfo, anyhow::Error> {
    let status = model.effective_status_now();

    Ok(TransactionRequestInfo {
        request_id: model.id,
        transaction: model.unsigned_transaction,
        seal_signer: model.seal_signer,
        other_signers: model.other_signers,
        requested_by: model.requested_by,
        status,
        transaction_id: model.transaction_id,
        value_summary: value_summary(context, &model.lock_ids)?,
        expires_at: model.expires_at.assume_utc().unix_timestamp(),
        approved_at: model.approved_at.map(|t| t.assume_utc().unix_timestamp()),
        created_at: model.created_at.assume_utc().unix_timestamp(),
    })
}

/// What actually leaves the wallet, derived from the request's locks.
///
/// A stealth transfer's instructions are commitments and range proofs; the
/// wallet owns the masks and is the only party that can say "10,000 µT leaves
/// this account". Without this the approver is authorising an opaque blob.
///
/// `None` for an ordinary transaction, whose instructions are readable as-is.
fn value_summary(
    context: &HandlerContext,
    lock_ids: &[WalletLockId],
) -> Result<Option<TransactionRequestValueSummary>, anyhow::Error> {
    let sdk = context.wallet_sdk();
    let mut inputs_total = 0u64;
    let mut change_total = 0u64;
    let mut resource_address = None;

    for lock_id in lock_ids {
        let Some(outputs) = sdk.stealth_outputs_api().get_locked_by_lock_id(*lock_id).optional()? else {
            continue;
        };
        for output in outputs {
            resource_address.get_or_insert(output.resource_address);
            match output.status {
                // What this request spends.
                OutputStatus::LockedForSpend => inputs_total = inputs_total.saturating_add(output.value),
                // Outputs the statement created. Only the ones we hold a spend
                // key for are change coming back; the rest belong to the
                // recipient and are genuinely leaving.
                OutputStatus::LockedUnconfirmed if output.owner_key_id.is_some() => {
                    change_total = change_total.saturating_add(output.value)
                },
                _ => {},
            }
        }
    }

    let Some(resource_address) = resource_address else {
        return Ok(None);
    };

    Ok(Some(TransactionRequestValueSummary {
        resource_address,
        inputs_total,
        change_total,
        amount_leaving: inputs_total.saturating_sub(change_total),
    }))
}
