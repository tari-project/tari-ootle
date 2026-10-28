//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Create / approve / submit for transaction requests (issue #2343).
//!
//! A limited-permission tool creates a request; an interactive user session
//! holding `transaction_requests:approve` authorises it; submit seals it. The
//! three are separately permissioned, and approval refuses API keys outright,
//! so a tool cannot approve the requests it creates.
//!
//! The transaction is **frozen at creation**: stored verbatim (the caller
//! supplies complete inputs and any out-of-band signatures). The approver views
//! exactly what will be sealed, and submit seals it unchanged.

use std::{collections::HashSet, time::Duration};

use axum_extra::headers::authorization::Bearer;
use indexmap::IndexMap;
use log::*;
use ootle_byte_type::ToByteType;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_transaction::{Instruction, MainSigner, ResourceAddressRef, UnsignedTransaction};
use tari_ootle_wallet_sdk::{
    models::{
        EffectiveStatus,
        KeyId,
        LockOwner,
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
        TransactionRequestMainSigner,
        TransactionRequestSubmitRequest,
        TransactionRequestSubmitResponse,
        TransactionRequestValueSummary,
        TransactionSubmitRequest,
    },
};
use tari_template_lib_types::{ResourceAddress, crypto::PedersenCommitmentBytes, stealth::SpendAuthorization};

use super::context::HandlerContext;
use crate::handlers::{
    helpers::{ensure_transaction_signers, invalid_params, invalid_request},
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
    ensure_transaction_signers(sdk, &req.seal_signer, &req.other_signers)?;
    if let TransactionRequestMainSigner::External { public_key } =
        resolve_main_signer(context, &req.transaction, &req.other_signers)? &&
        !req.signatures.iter().any(|sig| *sig.public_key() == public_key)
    {
        return Err(invalid_params(
            "transaction.main_signer",
            Some(format!(
                "names {public_key} as the main signer, but neither other_signers nor signatures sign with \
                 {public_key}"
            )),
        ));
    }

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

        ensure_free_locks_of(&mut *tx, auth.lock_owner(), &req.lock_ids, "lock_ids")?;

        // Hold the inputs across the approval window. The transfer-selection
        // handlers lock for five minutes, which is a selection deadline, not a
        // deadline a person can meet.
        for lock_id in &req.lock_ids {
            tx.locks_extend_timeout(*lock_id, ttl + LOCK_GRACE)?;
        }

        let requested_by = auth.requested_by();
        let model = tx.transaction_request_insert(
            &unsigned,
            req.seal_signer,
            &req.other_signers,
            &req.signatures,
            &req.lock_ids,
            requested_by.as_deref(),
            ttl,
        )?;
        tx.commit()?;
        model
    };

    info!(
        target: LOG_TARGET,
        "Transaction request {} created by {} ({} lock(s), expires {})",
        model.id,
        auth.signing_requester(),
        req.lock_ids.len(),
        model.expires_at,
    );

    context
        .notifier()
        .notify(WalletEvent::TransactionRequestCreated(TransactionRequestCreatedEvent {
            request_id: model.id,
            requested_by: auth.requested_by(),
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
    // Approving authorises the value transfer the request describes, so it is
    // a human decision: it takes an interactive user session, and no API key
    // can approve whatever it was granted.
    context.authorize_user_only(token, &[Permission::TransactionRequests(TxRequestAction::Approve)])?;

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

/// Refuses `lock_ids` unless `caller` created each lock and neither a
/// transaction nor a live request holds it. Rejecting a request releases its
/// locks, so a lock held by two requests would be released under the other.
///
/// Every refusal is the same error on `field`, so the call does not reveal
/// which lock ids exist.
pub(crate) fn ensure_free_locks_of<R: WalletStoreReader>(
    reader: &mut R,
    caller: LockOwner,
    lock_ids: &[WalletLockId],
    field: &'static str,
) -> Result<(), anyhow::Error> {
    if lock_ids.is_empty() {
        return Ok(());
    }
    let mut held = reader
        .transaction_requests_list()?
        .into_iter()
        .filter(|r| {
            matches!(
                r.effective_status_now(),
                EffectiveStatus::Pending | EffectiveStatus::Approved | EffectiveStatus::Submitting
            )
        })
        .flat_map(|r| r.lock_ids)
        .collect::<HashSet<_>>();

    for lock_id in lock_ids {
        let lock = reader.locks_get(*lock_id).optional()?;
        let free = lock.is_some_and(|l| l.owner == caller && l.transaction_id.is_none());
        if !free || !held.insert(*lock_id) {
            return Err(invalid_params(
                field,
                Some(format!(
                    "lock {lock_id} does not exist, was created by another caller, or is already in use"
                )),
            ));
        }
    }
    Ok(())
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
    let value_summaries = value_summaries(context, &model.unsigned_transaction, status, &model.lock_ids)?;

    let main_signer = resolve_main_signer(context, &model.unsigned_transaction, &model.other_signers)?;
    let external_signers = model.signatures.iter().map(|sig| *sig.public_key()).collect();

    Ok(TransactionRequestInfo {
        request_id: model.id,
        transaction: model.unsigned_transaction,
        seal_signer: model.seal_signer,
        other_signers: model.other_signers,
        requested_by: model.requested_by,
        status,
        transaction_id: model.transaction_id,
        value_summaries,
        main_signer,
        external_signers,
        expires_at: model.expires_at.assume_utc().unix_timestamp(),
        approved_at: model.approved_at.map(|t| t.assume_utc().unix_timestamp()),
        created_at: model.created_at.assume_utc().unix_timestamp(),
    })
}

/// The transaction's declared main signer, placed among the keys of this wallet.
fn resolve_main_signer(
    context: &HandlerContext,
    transaction: &UnsignedTransaction,
    other_signers: &[KeyId],
) -> Result<TransactionRequestMainSigner, anyhow::Error> {
    let public_key = match transaction.main_signer() {
        MainSigner::SealSigner => return Ok(TransactionRequestMainSigner::SealSigner),
        MainSigner::None => return Ok(TransactionRequestMainSigner::None),
        MainSigner::Authorizer(public_key) => *public_key,
    };
    let key_manager = context.wallet_sdk().key_manager_api();
    for key_id in other_signers {
        if key_manager.get_public_key(*key_id)?.public_key.to_byte_type() == public_key {
            return Ok(TransactionRequestMainSigner::WalletKey { public_key });
        }
    }
    Ok(TransactionRequestMainSigner::External { public_key })
}

/// What the transaction's stealth transfers move out of the wallet, one
/// summary per resource.
///
/// A stealth transfer's instructions are commitments and range proofs; the
/// wallet owns the masks and is the only party that can say "10,000 µT leaves
/// this account". The figures are derived from the statements in the
/// transaction being approved, which are what the engine executes.
fn value_summaries(
    context: &HandlerContext,
    transaction: &UnsignedTransaction,
    status: EffectiveStatus,
    lock_ids: &[WalletLockId],
) -> Result<Vec<TransactionRequestValueSummary>, anyhow::Error> {
    // A workspace reference resolves only at execution, so its commitments
    // match a wallet output of any resource.
    let mut spent = Vec::new();
    let mut created = Vec::new();
    for instruction in transaction.fee_instructions().iter().chain(transaction.instructions()) {
        let Instruction::StealthTransfer {
            resource_address_ref,
            statement,
            ..
        } = instruction
        else {
            continue;
        };
        let resource = match resource_address_ref {
            ResourceAddressRef::Address(address) => Some(*address),
            ResourceAddressRef::Workspace(_) => None,
        };
        spent.extend(
            statement
                .inputs_statement
                .inputs
                .iter()
                .map(|i| (resource, i.commitment)),
        );
        created.extend(statement.outputs_statement.outputs.iter().map(|o| (resource, o)));
    }
    if spent.is_empty() {
        return Ok(Vec::new());
    }

    let commitments = spent
        .iter()
        .map(|(_, c)| *c)
        .chain(created.iter().map(|(_, o)| *o.commitment()))
        .collect::<Vec<_>>();
    let known = context
        .wallet_sdk()
        .store()
        .with_read_tx(|tx| tx.stealth_outputs_get_by_commitments(&commitments))?;
    let wallet_outputs = |resource: Option<ResourceAddress>, commitment: PedersenCommitmentBytes| {
        known
            .iter()
            .filter(move |o| o.commitment == commitment && resource.is_none_or(|r| r == o.resource_address))
    };

    // (inputs_total, change_total) per resource, in transaction order.
    let mut totals = IndexMap::<ResourceAddress, (u64, u64)>::new();
    let mut inputs = HashSet::new();
    for (resource, commitment) in spent {
        for output in wallet_outputs(resource, commitment) {
            if inputs.insert(output.to_utxo_address()) {
                let (inputs_total, _) = totals.entry(output.resource_address).or_default();
                *inputs_total = inputs_total.saturating_add(output.value);
            }
        }
    }

    let awaiting_submission = !matches!(status, EffectiveStatus::Submitted);
    let mut change = HashSet::new();
    for (resource, created) in created {
        // An output under a workspace reference resolves its resource at execution time,
        // so change is attributed only when the statement names an explicit resource address.
        let Some(resource) = resource else {
            continue;
        };
        for output in wallet_outputs(Some(resource), *created.commitment()) {
            // Change is a pending output recorded under one of the request's locks while the
            // request awaits submission, spendable by the wallet's key alone, and created with
            // exactly the authorisation and encrypted opening the wallet recorded. Once a
            // request has been submitted, finalizing its lock promotes the change output to
            // Unspent and clears lock_id, so the pending-lock check applies only beforehand.
            let lock_and_status_match = !awaiting_submission ||
                (matches!(output.status, OutputStatus::LockedUnconfirmed) &&
                    output.lock_id.is_some_and(|id| lock_ids.contains(&id)));
            let returns_to_wallet = lock_and_status_match &&
                output.owner_key_id.is_some() &&
                matches!(output.auth, SpendAuthorization::Key(_)) &&
                output.auth == created.auth &&
                output.encrypted_data == created.output.encrypted_data &&
                output.sender_public_nonce == created.output.sender_public_nonce;
            let address = output.to_utxo_address();
            if !returns_to_wallet || inputs.contains(&address) || !change.insert(address) {
                continue;
            }
            // Change offsets only a resource the transaction spends from this
            // wallet; funds received in any other resource leave nothing.
            if let Some((_, change_total)) = totals.get_mut(&output.resource_address) {
                *change_total = change_total.saturating_add(output.value);
            }
        }
    }

    Ok(totals
        .into_iter()
        .map(
            |(resource_address, (inputs_total, change_total))| TransactionRequestValueSummary {
                resource_address,
                inputs_total,
                change_total,
                amount_leaving: inputs_total.saturating_sub(change_total),
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use tari_ootle_address::Network;
    use tari_ootle_common_types::{Epoch, crypto::create_key_pair, optional::IsNotFoundError};
    use tari_ootle_transaction::TransactionSignature;
    use tari_ootle_wallet_sdk::models::{KeyBranch, KeyId, KeyType, LockOwner, WalletLock};

    use super::*;
    use crate::handlers::test_support::TestDaemon;

    fn insert_request(daemon: &TestDaemon) -> TransactionRequestId {
        daemon
            .context
            .wallet_sdk()
            .store()
            .with_write_tx(|tx| {
                tx.transaction_request_insert(
                    &UnsignedTransaction::new(Network::LocalNet.as_byte(), Epoch(100)),
                    KeyId::derived(KeyBranch::Account, 0),
                    &[],
                    &[],
                    &[],
                    Some("htlc-swap-tool"),
                    Duration::from_secs(600),
                )
            })
            .unwrap()
            .id
    }

    fn status_of(daemon: &TestDaemon, request_id: TransactionRequestId) -> TransactionRequestStatus {
        daemon
            .context
            .wallet_sdk()
            .store()
            .with_read_tx(|tx| tx.transaction_request_get(request_id))
            .unwrap()
            .status
    }

    #[tokio::test]
    async fn only_the_wallets_own_session_can_approve() {
        let daemon = TestDaemon::start().await;
        let request_id = insert_request(&daemon);
        let approve = |bearer| {
            handle_approve(&daemon.context, Some(bearer), TransactionRequestDecisionRequest {
                request_id,
            })
        };

        // A key minted before `transaction_requests:approve` stopped being grantable.
        let api_key = daemon.api_key("htlc-swap-tool", "transaction_requests:approve");
        let err = approve(&api_key).await.unwrap_err();
        assert!(err.to_string().contains("interactive user session"), "{err}");

        let admin_key = daemon.api_key("admin-tool", "admin");
        approve(&admin_key).await.unwrap_err();

        // What an API key holding `webrtc` gets back from `webrtc.start`.
        let delegated = daemon.delegated_session("admin");
        let err = approve(&delegated).await.unwrap_err();
        assert!(err.to_string().contains("interactive user session"), "{err}");

        assert_eq!(status_of(&daemon, request_id), TransactionRequestStatus::Pending);

        approve(&daemon.session).await.unwrap();
        assert_eq!(status_of(&daemon, request_id), TransactionRequestStatus::Approved);
    }

    fn create_request(lock_ids: Vec<WalletLockId>, ttl_secs: u64) -> TransactionRequestCreateRequest {
        TransactionRequestCreateRequest {
            transaction: UnsignedTransaction::new(Network::LocalNet.as_byte(), Epoch(100)),
            seal_signer: KeyId::derived(KeyBranch::Account, 0),
            other_signers: vec![],
            signatures: vec![],
            lock_ids,
            ttl_secs: Some(ttl_secs),
        }
    }

    async fn request_info(daemon: &TestDaemon, request_id: TransactionRequestId) -> TransactionRequestInfo {
        handle_get(&daemon.context, Some(&daemon.session), TransactionRequestGetRequest {
            request_id,
        })
        .await
        .unwrap()
        .request
    }

    #[tokio::test]
    async fn the_approver_sees_who_the_transaction_acts_as() {
        let daemon = TestDaemon::start().await;
        let tool = daemon.api_key("htlc-swap-tool", "transaction_requests:create");
        let wallet_key = KeyId::derived(KeyBranch::Account, 0);
        let wallet_public_key = daemon
            .context
            .wallet_sdk()
            .key_manager_api()
            .get_public_key(wallet_key)
            .unwrap()
            .public_key
            .to_byte_type();
        let (external_secret, external_public_key) = create_key_pair();
        let external_public_key = external_public_key.to_byte_type();

        let mut req = create_request(vec![], 600);
        req.transaction
            .set_main_signer(MainSigner::Authorizer(external_public_key));
        let err = handle_create(&daemon.context, Some(&tool), req.clone())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("transaction.main_signer"), "{err}");

        req.signatures = vec![TransactionSignature::sign(
            &external_secret,
            &wallet_public_key,
            &req.transaction,
        )];
        let request_id = handle_create(&daemon.context, Some(&tool), req)
            .await
            .unwrap()
            .request_id;
        let info = request_info(&daemon, request_id).await;
        assert_eq!(info.main_signer, TransactionRequestMainSigner::External {
            public_key: external_public_key
        });
        assert_eq!(info.external_signers, vec![external_public_key]);

        let mut req = create_request(vec![], 600);
        req.other_signers = vec![wallet_key];
        req.transaction
            .set_main_signer(MainSigner::Authorizer(wallet_public_key));
        let request_id = handle_create(&daemon.context, Some(&tool), req)
            .await
            .unwrap()
            .request_id;
        let info = request_info(&daemon, request_id).await;
        assert_eq!(info.main_signer, TransactionRequestMainSigner::WalletKey {
            public_key: wallet_public_key
        });
        assert!(info.external_signers.is_empty());
    }

    fn lock_for(daemon: &TestDaemon, owner: LockOwner) -> WalletLockId {
        daemon
            .context
            .wallet_sdk()
            .locks_api()
            .create_owned_lock_with_timeout(Duration::from_secs(5 * 60), owner)
            .unwrap()
            .keep_locked()
    }

    fn get_lock(daemon: &TestDaemon, lock_id: WalletLockId) -> WalletLock {
        daemon
            .context
            .wallet_sdk()
            .store()
            .with_read_tx(|tx| tx.locks_get(lock_id))
            .unwrap()
    }

    fn owner_of(daemon: &TestDaemon, bearer: &Bearer) -> LockOwner {
        daemon
            .context
            .check_auth_with_identity(Some(bearer))
            .unwrap()
            .lock_owner()
    }

    #[tokio::test]
    async fn a_request_cannot_hold_a_lock_another_caller_created() {
        let daemon = TestDaemon::start().await;
        let tool = daemon.api_key("htlc-swap-tool", "transaction_requests:create");
        let other_tool = daemon.api_key("other-tool", "transaction_requests:create");
        let app = daemon.delegated_session("transaction_requests:create");
        let foreign = [
            lock_for(&daemon, LockOwner::Wallet),
            lock_for(&daemon, owner_of(&daemon, &daemon.cookie)),
            lock_for(&daemon, owner_of(&daemon, &other_tool)),
            lock_for(&daemon, owner_of(&daemon, &app)),
        ];

        // A cookie caller's locks are distinct from the interactive wallet session's locks.
        let wallet_lock = lock_for(&daemon, LockOwner::Wallet);
        let err = handle_create(
            &daemon.context,
            Some(&daemon.cookie),
            create_request(vec![wallet_lock], 60 * 60),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("lock_ids"), "{err}");

        let cookie_lock = lock_for(&daemon, owner_of(&daemon, &daemon.cookie));
        let err = handle_create(
            &daemon.context,
            Some(&daemon.session),
            create_request(vec![cookie_lock], 60 * 60),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("lock_ids"), "{err}");

        let cookie_req_id = handle_create(
            &daemon.context,
            Some(&daemon.cookie),
            create_request(vec![cookie_lock], 60 * 60),
        )
        .await
        .unwrap()
        .request_id;
        assert_eq!(
            request_info(&daemon, cookie_req_id).await.requested_by.as_deref(),
            Some("local-cookie")
        );

        for lock_id in foreign {
            let deadline = get_lock(&daemon, lock_id).timeout_at;
            let err = handle_create(&daemon.context, Some(&tool), create_request(vec![lock_id], 60 * 60))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("lock_ids"), "{err}");
            assert_eq!(
                get_lock(&daemon, lock_id).timeout_at,
                deadline,
                "lock {lock_id} was moved"
            );
        }

        // A missing id is refused the same way as a foreign one.
        let missing = handle_create(&daemon.context, Some(&tool), create_request(vec![9999], 60 * 60))
            .await
            .unwrap_err();
        let foreign_err = handle_create(&daemon.context, Some(&tool), create_request(vec![foreign[0]], 60 * 60))
            .await
            .unwrap_err();
        assert_eq!(
            missing.to_string().replace("9999", "N"),
            foreign_err.to_string().replace(&foreign[0].to_string(), "N")
        );
    }

    #[tokio::test]
    async fn connected_apps_cannot_hold_each_others_locks() {
        let daemon = TestDaemon::start().await;
        let app = daemon.delegated_session("transaction_requests:create");
        let other_app = daemon.delegated_session("transaction_requests:create");
        let lock_id = lock_for(&daemon, owner_of(&daemon, &other_app));

        let err = handle_create(&daemon.context, Some(&app), create_request(vec![lock_id], 60 * 60))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("lock_ids"), "{err}");

        handle_create(
            &daemon.context,
            Some(&other_app),
            create_request(vec![lock_id], 60 * 60),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_request_holds_its_own_callers_lock_across_the_approval_window() {
        let daemon = TestDaemon::start().await;
        let tool = daemon.api_key("htlc-swap-tool", "transaction_requests:create");
        let lock_id = lock_for(&daemon, owner_of(&daemon, &tool));
        let deadline = get_lock(&daemon, lock_id).timeout_at.unwrap();

        handle_create(&daemon.context, Some(&tool), create_request(vec![lock_id], 60 * 60))
            .await
            .unwrap();

        let extended = get_lock(&daemon, lock_id).timeout_at.unwrap();
        assert!(
            extended - deadline >= time::Duration::minutes(55),
            "{deadline} -> {extended}"
        );
    }

    #[tokio::test]
    async fn a_lock_is_held_by_at_most_one_live_request() {
        let daemon = TestDaemon::start().await;
        let tool = daemon.api_key("htlc-swap-tool", "transaction_requests:create");
        let lock_id = lock_for(&daemon, owner_of(&daemon, &tool));

        let first = handle_create(&daemon.context, Some(&tool), create_request(vec![lock_id], 60 * 60))
            .await
            .unwrap();
        handle_create(&daemon.context, Some(&tool), create_request(vec![lock_id], 60 * 60))
            .await
            .unwrap_err();
        handle_create(
            &daemon.context,
            Some(&tool),
            create_request(vec![lock_for(&daemon, owner_of(&daemon, &tool)); 2], 60 * 60),
        )
        .await
        .unwrap_err();

        // The first request's rejection releases the lock, so nothing holds it.
        handle_reject(
            &daemon.context,
            Some(&daemon.session),
            TransactionRequestDecisionRequest {
                request_id: first.request_id,
            },
        )
        .await
        .unwrap();
        let released = daemon
            .context
            .wallet_sdk()
            .store()
            .with_read_tx(|tx| tx.locks_get(lock_id))
            .unwrap_err();
        assert!(released.is_not_found_error(), "{released}");
    }

    #[tokio::test]
    async fn a_request_never_brings_a_lock_deadline_forward() {
        let daemon = TestDaemon::start().await;
        let lock_id = daemon
            .context
            .wallet_sdk()
            .locks_api()
            .create_lock()
            .unwrap()
            .keep_locked();

        handle_create(&daemon.context, Some(&daemon.session), create_request(vec![lock_id], 0))
            .await
            .unwrap();

        assert_eq!(get_lock(&daemon, lock_id).timeout_at, None);
    }

    #[tokio::test]
    async fn a_request_cannot_name_a_mask_or_view_key_as_a_signer() {
        let daemon = TestDaemon::start().await;
        let tool = daemon.api_key("htlc-swap-tool", "transaction_requests:create");

        for branch in [
            KeyBranch::ConfidentialMask,
            KeyBranch::StealthMask,
            KeyBranch::ViewOnlyKey,
            KeyBranch::ElgamalEncryptionViewKey,
        ] {
            let mut req = create_request(vec![], 60 * 60);
            req.other_signers = vec![KeyId::derived(branch, 0)];
            let err = handle_create(&daemon.context, Some(&tool), req).await.unwrap_err();
            assert!(err.to_string().contains("other_signers"), "{err}");

            let mut req = create_request(vec![], 60 * 60);
            req.seal_signer = KeyId::derived(branch, 0);
            let err = handle_create(&daemon.context, Some(&tool), req).await.unwrap_err();
            assert!(err.to_string().contains("seal_signer"), "{err}");
        }

        let stored = daemon
            .context
            .wallet_sdk()
            .store()
            .with_read_tx(|tx| tx.transaction_requests_list())
            .unwrap();
        assert!(stored.is_empty(), "a refused request was stored");

        let mut req = create_request(vec![], 60 * 60);
        req.other_signers = vec![KeyId::derived(KeyBranch::Nonce, 0)];
        handle_create(&daemon.context, Some(&tool), req).await.unwrap();
    }

    #[tokio::test]
    async fn a_request_cannot_name_a_view_only_accounts_view_key_as_a_signer() {
        let daemon = TestDaemon::start().await;
        let tool = daemon.api_key("htlc-swap-tool", "transaction_requests:create");
        let (_, account_public_key) = create_key_pair();
        let (view_secret_key, _) = create_key_pair();
        let account = daemon
            .context
            .wallet_sdk()
            .accounts_api()
            .add_view_only_account(
                Some("watch"),
                &account_public_key.to_byte_type(),
                &view_secret_key,
                None,
            )
            .unwrap();
        let view_key_id = account.view_only_key_id();
        assert!(matches!(view_key_id, KeyId::Imported { .. }));

        let mut req = create_request(vec![], 60 * 60);
        req.other_signers = vec![view_key_id];
        let err = handle_create(&daemon.context, Some(&tool), req).await.unwrap_err();
        assert!(err.to_string().contains("other_signers"), "{err}");

        let mut req = create_request(vec![], 60 * 60);
        req.seal_signer = view_key_id;
        let err = handle_create(&daemon.context, Some(&tool), req).await.unwrap_err();
        assert!(err.to_string().contains("seal_signer"), "{err}");

        let (secret, _) = create_key_pair();
        let general_key_id = daemon
            .context
            .wallet_sdk()
            .key_manager_api()
            .import_key("general", &secret, KeyType::GeneralPurpose)
            .unwrap();
        let mut req = create_request(vec![], 60 * 60);
        req.other_signers = vec![general_key_id];
        handle_create(&daemon.context, Some(&tool), req).await.unwrap();
    }

    mod value_summaries {
        use tari_ootle_transaction::{TransactionBuilder, args::WorkspaceOffsetId};
        use tari_ootle_wallet_sdk::models::StealthOutputModel;
        use tari_template_lib_types::{
            Amount,
            ComponentAddress,
            EncryptedData,
            Hash32,
            crypto::{RangeProofBytes, RistrettoPublicKeyBytes, UtxoTag},
            stealth::{
                StealthInput,
                StealthInputsStatement,
                StealthOutputsStatement,
                StealthTransferStatement,
                StealthUnspentOutput,
                UnspentOutput,
            },
        };

        use super::*;

        const ACCOUNT: &str = "component_0dc41b5cc74b36d696c7b140323a40a2f98b71df5d60e5a6bf4c1a07ffffffff";
        const RESOURCE: &str = "resource_0000000000000000000000000000000000000000000000000000000000000abc";

        fn account() -> ComponentAddress {
            ACCOUNT.parse().unwrap()
        }

        fn resource() -> ResourceAddress {
            RESOURCE.parse().unwrap()
        }

        fn commitment(n: u8) -> PedersenCommitmentBytes {
            PedersenCommitmentBytes::from_array([n; PedersenCommitmentBytes::length()])
        }

        fn wallet_output(n: u8, value: u64, status: OutputStatus) -> StealthOutputModel {
            StealthOutputModel {
                owner_account: account(),
                resource_address: resource(),
                commitment: commitment(n),
                value,
                sender_public_nonce: RistrettoPublicKeyBytes::default(),
                view_only_key_id: KeyId::derived(KeyBranch::ViewOnlyKey, 0),
                owner_key_id: Some(KeyId::derived(KeyBranch::Account, 0)),
                encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
                tag_byte: UtxoTag::new(n.into()),
                memo: None,
                auth: SpendAuthorization::Key(RistrettoPublicKeyBytes::default()),
                minimum_value_promise: 0,
                status,
                is_burnt: false,
                is_frozen: false,
                is_on_chain: true,
                is_condition_spendable: true,
                lock_id: None,
                proven_up: None,
                spent_version: None,
            }
        }

        fn as_statement_output(output: &StealthOutputModel) -> StealthUnspentOutput {
            StealthUnspentOutput {
                output: UnspentOutput {
                    commitment: output.commitment,
                    sender_public_nonce: output.sender_public_nonce,
                    encrypted_data: output.encrypted_data.clone(),
                    minimum_value_promise: output.minimum_value_promise,
                    viewable_balance_proof: None,
                },
                auth: output.auth.clone(),
                tag: output.tag_byte,
            }
        }

        fn stealth_transfer(
            resource_address_ref: ResourceAddressRef,
            inputs: &[PedersenCommitmentBytes],
            outputs: Vec<StealthUnspentOutput>,
        ) -> Instruction {
            Instruction::StealthTransfer {
                resource_address_ref,
                statement: StealthTransferStatement {
                    inputs_statement: StealthInputsStatement {
                        inputs: inputs.iter().copied().map(StealthInput::new).collect(),
                        revealed_amount: Amount::ZERO,
                    },
                    outputs_statement: StealthOutputsStatement {
                        outputs,
                        revealed_output: None,
                        agg_range_proof: RangeProofBytes::empty(),
                    },
                    balance_proof: None,
                    covenant_claims: vec![],
                },
                revealed_input_bucket: None,
            }
        }

        fn with_instructions(instructions: Vec<Instruction>) -> UnsignedTransaction {
            TransactionBuilder::new(Network::LocalNet.as_byte(), Epoch(100))
                .without_auto_fill_inputs()
                .with_instructions(instructions)
                .build_unsigned()
        }

        struct Wallet {
            daemon: TestDaemon,
            tool: Bearer,
        }

        impl Wallet {
            async fn start() -> Self {
                let daemon = TestDaemon::start().await;
                daemon
                    .context
                    .wallet_sdk()
                    .accounts_api()
                    .add_account(
                        Some("default"),
                        &account(),
                        KeyId::derived(KeyBranch::ViewOnlyKey, 0),
                        KeyId::derived(KeyBranch::Account, 0),
                        Epoch::zero(),
                        true,
                        true,
                    )
                    .unwrap();
                let tool = daemon.api_key("swap-tool", "transaction_requests:create");
                Self { daemon, tool }
            }

            fn add_output(&self, output: &StealthOutputModel) {
                self.daemon
                    .context
                    .wallet_sdk()
                    .stealth_outputs_api()
                    .add_output(output)
                    .unwrap();
            }

            /// A lock the tool owns over `output`, as a statement-building handler would leave it.
            fn tool_lock_over(&self, output: &StealthOutputModel) -> WalletLockId {
                let lock_id = lock_for(&self.daemon, owner_of(&self.daemon, &self.tool));
                self.daemon
                    .context
                    .wallet_sdk()
                    .stealth_outputs_api()
                    .lock_specific_outputs(&account(), &resource(), lock_id, &[output.to_utxo_address()])
                    .unwrap();
                lock_id
            }

            async fn info(
                &self,
                transaction: UnsignedTransaction,
                lock_ids: Vec<WalletLockId>,
            ) -> TransactionRequestInfo {
                let mut req = create_request(lock_ids, 60 * 60);
                req.transaction = transaction;
                let created = handle_create(&self.daemon.context, Some(&self.tool), req)
                    .await
                    .unwrap();
                handle_get(
                    &self.daemon.context,
                    Some(&self.daemon.session),
                    TransactionRequestGetRequest {
                        request_id: created.request_id,
                    },
                )
                .await
                .unwrap()
                .request
            }

            /// `(inputs_total, change_total, amount_leaving)` of a one-statement transaction.
            async fn totals(
                &self,
                inputs: &[PedersenCommitmentBytes],
                outputs: Vec<StealthUnspentOutput>,
                lock_ids: Vec<WalletLockId>,
            ) -> (u64, u64, u64) {
                let tx = with_instructions(vec![stealth_transfer(
                    ResourceAddressRef::Address(resource()),
                    inputs,
                    outputs,
                )]);
                let info = self.info(tx, lock_ids).await;
                let [summary] = info.value_summaries.as_slice() else {
                    panic!("expected one summary: {:?}", info.value_summaries);
                };
                (summary.inputs_total, summary.change_total, summary.amount_leaving)
            }
        }

        #[tokio::test]
        async fn it_counts_the_inputs_the_transaction_spends_not_the_requests_locks() {
            let wallet = Wallet::start().await;
            let small = wallet_output(1, 1, OutputStatus::Unspent);
            let large = wallet_output(2, 1_000_000, OutputStatus::Unspent);
            wallet.add_output(&small);
            wallet.add_output(&large);
            let lock_id = wallet.tool_lock_over(&small);

            let tx = with_instructions(vec![stealth_transfer(
                ResourceAddressRef::Address(resource()),
                &[large.commitment],
                vec![],
            )]);
            let info = wallet.info(tx, vec![lock_id]).await;

            let [summary] = info.value_summaries.as_slice() else {
                panic!("expected one summary: {:?}", info.value_summaries);
            };
            assert_eq!(summary.resource_address, resource());
            assert_eq!(summary.inputs_total, 1_000_000);
            assert_eq!(summary.change_total, 0);
            assert_eq!(summary.amount_leaving, 1_000_000);
        }

        #[tokio::test]
        async fn a_lock_the_transaction_does_not_spend_states_no_value() {
            let wallet = Wallet::start().await;
            let small = wallet_output(1, 1, OutputStatus::Unspent);
            wallet.add_output(&small);
            let lock_id = wallet.tool_lock_over(&small);

            let info = wallet
                .info(UnsignedTransaction::new(Network::LocalNet.as_byte(), Epoch(100)), vec![
                    lock_id,
                ])
                .await;

            assert!(info.value_summaries.is_empty(), "{:?}", info.value_summaries);
        }

        #[tokio::test]
        async fn it_counts_inputs_whose_resource_is_resolved_from_the_workspace() {
            let wallet = Wallet::start().await;
            let large = wallet_output(2, 1_000_000, OutputStatus::Unspent);
            wallet.add_output(&large);

            let tx = with_instructions(vec![stealth_transfer(
                ResourceAddressRef::Workspace(WorkspaceOffsetId::new(0)),
                &[large.commitment],
                vec![],
            )]);
            let info = wallet.info(tx, vec![]).await;

            let [summary] = info.value_summaries.as_slice() else {
                panic!("expected one summary: {:?}", info.value_summaries);
            };
            assert_eq!(summary.resource_address, resource());
            assert_eq!(summary.amount_leaving, 1_000_000);
        }

        #[tokio::test]
        async fn change_is_an_output_created_exactly_as_the_wallet_recorded_it() {
            let wallet = Wallet::start().await;
            let lock_id = lock_for(&wallet.daemon, owner_of(&wallet.daemon, &wallet.tool));
            let mut input = wallet_output(2, 1_000_000, OutputStatus::LockedForSpend);
            input.lock_id = Some(lock_id);
            let mut change = wallet_output(3, 400_000, OutputStatus::LockedUnconfirmed);
            change.lock_id = Some(lock_id);
            wallet.add_output(&input);
            wallet.add_output(&change);

            let mut redirected = as_statement_output(&change);
            redirected.auth = SpendAuthorization::Key(RistrettoPublicKeyBytes::from_bytes(&[9; 32]).unwrap());
            let mut not_ours = wallet_output(4, 300_000, OutputStatus::LockedUnconfirmed);
            not_ours.lock_id = Some(lock_id);
            not_ours.owner_key_id = None;
            wallet.add_output(&not_ours);

            let spend = &[input.commitment];
            assert_eq!(
                wallet
                    .totals(spend, vec![as_statement_output(&change)], vec![lock_id])
                    .await,
                (1_000_000, 400_000, 600_000)
            );
            assert_eq!(
                wallet.totals(spend, vec![redirected], vec![lock_id]).await,
                (1_000_000, 0, 1_000_000)
            );
            assert_eq!(
                wallet
                    .totals(spend, vec![as_statement_output(&not_ours)], vec![lock_id])
                    .await,
                (1_000_000, 0, 1_000_000)
            );
            assert_eq!(
                wallet
                    .totals(spend, vec![as_statement_output(&input)], vec![lock_id])
                    .await,
                (1_000_000, 0, 1_000_000),
                "an output the transaction also spends was counted as change"
            );

            for (n, status) in [
                (5, OutputStatus::Unspent),
                (6, OutputStatus::Spent),
                (7, OutputStatus::LockedForSpend),
            ] {
                let mut existing = wallet_output(n, 1_000_000, status);
                existing.lock_id = Some(lock_id);
                wallet.add_output(&existing);
                assert_eq!(
                    wallet
                        .totals(spend, vec![as_statement_output(&existing)], vec![lock_id])
                        .await,
                    (1_000_000, 0, 1_000_000),
                    "a {status:?} wallet output was replayed as change"
                );
            }

            for (n, auth) in [
                (8, SpendAuthorization::Script(Hash32::zero())),
                (
                    9,
                    SpendAuthorization::KeyAndScript(RistrettoPublicKeyBytes::default(), Hash32::zero()),
                ),
            ] {
                let mut script_gated = wallet_output(n, 400_000, OutputStatus::LockedUnconfirmed);
                script_gated.lock_id = Some(lock_id);
                script_gated.auth = auth;
                wallet.add_output(&script_gated);
                assert_eq!(
                    wallet
                        .totals(spend, vec![as_statement_output(&script_gated)], vec![lock_id])
                        .await,
                    (1_000_000, 0, 1_000_000),
                    "a condition-tree output was counted as change"
                );
            }

            let mut foreign_locked = wallet_output(10, 400_000, OutputStatus::LockedUnconfirmed);
            foreign_locked.lock_id = Some(lock_for(&wallet.daemon, owner_of(&wallet.daemon, &wallet.tool)));
            wallet.add_output(&foreign_locked);
            assert_eq!(
                wallet
                    .totals(spend, vec![as_statement_output(&foreign_locked)], vec![lock_id])
                    .await,
                (1_000_000, 0, 1_000_000),
                "an unconfirmed output from an unheld lock was counted as change"
            );

            let unlocked_unconfirmed = wallet_output(11, 400_000, OutputStatus::LockedUnconfirmed);
            wallet.add_output(&unlocked_unconfirmed);
            assert_eq!(
                wallet
                    .totals(spend, vec![as_statement_output(&unlocked_unconfirmed)], vec![lock_id])
                    .await,
                (1_000_000, 0, 1_000_000),
                "an unconfirmed output with no lock_id was counted as change"
            );
        }

        #[tokio::test]
        async fn submitted_request_retains_change_summary_after_outputs_finalize() {
            let wallet = Wallet::start().await;
            let input = wallet_output(2, 1_000_000, OutputStatus::Spent);
            let change = wallet_output(3, 400_000, OutputStatus::Unspent);
            wallet.add_output(&input);
            wallet.add_output(&change);

            let tx = with_instructions(vec![stealth_transfer(
                ResourceAddressRef::Address(resource()),
                &[input.commitment],
                vec![as_statement_output(&change)],
            )]);
            let mut req = create_request(vec![], 60 * 60);
            req.transaction = tx;
            let created = handle_create(&wallet.daemon.context, Some(&wallet.tool), req)
                .await
                .unwrap();
            wallet
                .daemon
                .context
                .wallet_sdk()
                .store()
                .with_write_tx(|tx| {
                    tx.transaction_request_transition(
                        created.request_id,
                        &[TransactionRequestStatus::Pending],
                        TransactionRequestStatus::Submitted,
                    )
                })
                .unwrap();

            let info = handle_get(
                &wallet.daemon.context,
                Some(&wallet.daemon.session),
                TransactionRequestGetRequest {
                    request_id: created.request_id,
                },
            )
            .await
            .unwrap()
            .request;
            let [summary] = info.value_summaries.as_slice() else {
                panic!("expected one summary: {:?}", info.value_summaries);
            };
            assert_eq!(
                (summary.inputs_total, summary.change_total, summary.amount_leaving),
                (1_000_000, 400_000, 600_000)
            );
        }

        #[tokio::test]
        async fn workspace_resource_outputs_do_not_offset_spent_inputs_as_change() {
            let wallet = Wallet::start().await;
            let lock_id = lock_for(&wallet.daemon, owner_of(&wallet.daemon, &wallet.tool));
            let mut input = wallet_output(2, 1_000_000, OutputStatus::LockedForSpend);
            input.lock_id = Some(lock_id);
            let mut change = wallet_output(3, 1_000_000, OutputStatus::LockedUnconfirmed);
            change.lock_id = Some(lock_id);
            wallet.add_output(&input);
            wallet.add_output(&change);

            let tx = with_instructions(vec![
                stealth_transfer(ResourceAddressRef::Address(resource()), &[input.commitment], vec![]),
                stealth_transfer(
                    ResourceAddressRef::Workspace(WorkspaceOffsetId::new(0)),
                    &[],
                    vec![as_statement_output(&change)],
                ),
            ]);
            let info = wallet.info(tx, vec![lock_id]).await;

            let [summary] = info.value_summaries.as_slice() else {
                panic!("expected one summary: {:?}", info.value_summaries);
            };
            assert_eq!(summary.resource_address, resource());
            assert_eq!(summary.inputs_total, 1_000_000);
            assert_eq!(summary.change_total, 0);
            assert_eq!(summary.amount_leaving, 1_000_000);
        }

        #[tokio::test]
        async fn it_summarises_each_resource_separately() {
            let wallet = Wallet::start().await;
            let other: ResourceAddress = "resource_0000000000000000000000000000000000000000000000000000000000000def"
                .parse()
                .unwrap();
            let first = wallet_output(2, 1_000, OutputStatus::Unspent);
            let mut second = wallet_output(3, 7, OutputStatus::Unspent);
            second.resource_address = other;
            wallet.add_output(&first);
            wallet.add_output(&second);

            let tx = with_instructions(vec![
                stealth_transfer(ResourceAddressRef::Address(resource()), &[first.commitment], vec![]),
                stealth_transfer(ResourceAddressRef::Address(other), &[second.commitment], vec![]),
            ]);
            let info = wallet.info(tx, vec![]).await;

            let leaving = info
                .value_summaries
                .iter()
                .map(|s| (s.resource_address, s.amount_leaving))
                .collect::<Vec<_>>();
            assert_eq!(leaving, vec![(resource(), 1_000), (other, 7)]);
        }
    }
}
