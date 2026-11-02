//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    path::Path,
    time::Duration,
};

use anyhow::anyhow;
use ootle_byte_type::ToByteType;
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_app_utilities::burn_claim_proof::claim_proof_from_l1;
use tari_ootle_common_types::{
    SubstateRequirement,
    optional::{IsNotFoundError, Optional},
};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    WalletSdk,
    WalletSdkSpec,
    apis::accounts::{AccountsApi, AccountsApiError},
    models::{AccountWithAddress, DerivedKeyIndex, KeyId, TransactionFinalizedEvent, WalletEvent},
};
use tari_ootle_walletd_client::{
    ComponentAddressOrName,
    types::{ClaimBurnProofContents, UpgradableComponentInfo},
};
use tari_sidechain::CompleteClaimBurnProof;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib_types::{
    AccessRule,
    ComponentAddress,
    EncryptedData,
    SubstateOwnerRule,
    access_rules::UpdateRule,
};
use tokio::{sync::broadcast, time};

use crate::server::ApplicationErrorCode;

/// How long a handler waits for its transaction to finalize before answering that it is still pending.
const RESULT_WAIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub async fn wait_for_result(
    events: &mut broadcast::Receiver<WalletEvent>,
    transaction_id: TransactionId,
) -> Result<TransactionFinalizedEvent, anyhow::Error> {
    time::timeout(RESULT_WAIT_TIMEOUT, wait_for_result_unbounded(events, transaction_id))
        .await
        .map_err(|_| still_pending(transaction_id))?
}

async fn wait_for_result_unbounded(
    events: &mut broadcast::Receiver<WalletEvent>,
    transaction_id: TransactionId,
) -> Result<TransactionFinalizedEvent, anyhow::Error> {
    loop {
        let wallet_event = events.recv().await?;
        match wallet_event {
            WalletEvent::TransactionFinalized(event) if event.transaction_id == transaction_id => return Ok(event),
            WalletEvent::TransactionInvalid(event) if event.transaction_id == transaction_id => {
                return Err(anyhow::anyhow!(
                    "Transaction invalid: {} [status: {}]",
                    event
                        .finalize
                        .and_then(|finalize| finalize.fee_reject().cloned())
                        .map(|f| f.to_string())
                        .unwrap_or_else(|| "Unknown".to_string()),
                    event.status,
                ));
            },
            _ => {},
        }
    }
}

pub async fn wait_for_result_and_account(
    events: &mut broadcast::Receiver<WalletEvent>,
    transaction_id: &TransactionId,
    account_address: &ComponentAddress,
) -> Result<(TransactionFinalizedEvent, Option<ComponentAddress>), anyhow::Error> {
    time::timeout(
        RESULT_WAIT_TIMEOUT,
        wait_for_result_and_account_unbounded(events, transaction_id, account_address),
    )
    .await
    .map_err(|_| still_pending(*transaction_id))?
}

async fn wait_for_result_and_account_unbounded(
    events: &mut broadcast::Receiver<WalletEvent>,
    transaction_id: &TransactionId,
    account_address: &ComponentAddress,
) -> Result<(TransactionFinalizedEvent, Option<ComponentAddress>), anyhow::Error> {
    let mut maybe_account = None;
    let mut maybe_result = None;
    loop {
        let wallet_event = events.recv().await?;
        match wallet_event {
            WalletEvent::TransactionFinalized(event) if event.transaction_id == *transaction_id => {
                maybe_result = Some(event);
            },
            WalletEvent::TransactionInvalid(event) if event.transaction_id == *transaction_id => {
                return Err(anyhow::anyhow!(
                    "Transaction invalid: {} [status: {}]",
                    event
                        .finalize
                        .and_then(|finalize| finalize.fee_reject().cloned())
                        .map(|f| f.to_string())
                        .unwrap_or_else(|| "Unknown".to_string()),
                    event.status,
                ));
            },
            WalletEvent::AccountCreatedOnChain(event) if event.account.component_address == *account_address => {
                maybe_account = Some(event.account.component_address);
            },
            WalletEvent::AccountChangedOnChain(event) if event.account_address == *account_address => {
                maybe_account = Some(event.account_address);
            },
            _ => {},
        }
        if let Some(ref result) = maybe_result {
            // If accept, we wait for the account. If reject we return immediately
            if (result.finalize.result.is_any_accept() && maybe_account.is_some()) || result.finalize.result.is_reject()
            {
                return Ok((maybe_result.unwrap(), maybe_account));
            }
        }
    }
}

pub fn get_account_with_inputs<TSpec: WalletSdkSpec>(
    account: Option<&ComponentAddressOrName>,
    sdk: &WalletSdk<TSpec>,
) -> Result<(AccountWithAddress, HashSet<SubstateRequirement>), anyhow::Error> {
    let account = get_account_or_default(account, &sdk.accounts_api())?;
    let inputs = if account.is_confirmed_on_chain() {
        // Add all versioned account child addresses as inputs
        sdk.substate_api()
            .load_dependent_substates(&[&account.account.component_address.into()])?
    } else {
        HashSet::new()
    };

    Ok((account, inputs))
}

pub fn get_account<TSpec: WalletSdkSpec>(
    account: &ComponentAddressOrName,
    accounts_api: &AccountsApi<'_, TSpec>,
) -> Result<AccountWithAddress, AccountsApiError> {
    match account {
        ComponentAddressOrName::ComponentAddress(address) => Ok(accounts_api.get_account_by_address(address)?),
        ComponentAddressOrName::Name(name) => Ok(accounts_api.get_account_by_name(name)?),
    }
}

pub(crate) fn get_account_by_key_index<TSpec: WalletSdkSpec>(
    sdk: &WalletSdk<TSpec>,
    key_index: DerivedKeyIndex,
) -> Result<AccountWithAddress, AccountsApiError> {
    let key = sdk.key_manager_api().derive_account_address(key_index)?;
    let address =
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &key.address.account_key().to_byte_type());
    sdk.accounts_api().get_account_by_address(&address)
}

pub fn get_account_or_default<TSpec: WalletSdkSpec>(
    account: Option<&ComponentAddressOrName>,
    accounts_api: &AccountsApi<'_, TSpec>,
) -> Result<AccountWithAddress, anyhow::Error> {
    let result;
    if let Some(a) = account {
        result = get_account(a, accounts_api)
            .optional()?
            .ok_or_else(|| not_found(format!("Account '{a}' not found.",)))?;
    } else {
        result = accounts_api
            .get_default()
            .optional()?
            .ok_or_else(|| not_found("No default account found. Please create an account."))?;
    }
    Ok(result)
}

pub(super) fn invalid_params<T: Display>(field: &str, details: Option<T>) -> anyhow::Error {
    axum_jrpc::error::JsonRpcError::new(
        axum_jrpc::error::JsonRpcErrorReason::InvalidParams,
        format!(
            "Invalid param '{}'{}",
            field,
            details.map(|d| format!(": {}", d)).unwrap_or_default()
        ),
        serde_json::Value::Null,
    )
    .into()
}

/// Refuses a caller-named key unless it may sign (see [`KeyId::is_signing_key`]). The key manager
/// refuses the same keys when signing; checking here names the offending request field.
pub(super) fn ensure_signing_key<TSpec: WalletSdkSpec>(
    sdk: &WalletSdk<TSpec>,
    field: &str,
    key_id: &KeyId,
) -> Result<(), anyhow::Error> {
    if sdk.key_manager_api().is_signing_key(*key_id)? {
        return Ok(());
    }
    Err(invalid_params(
        field,
        Some(format!(
            "{key_id} is not a signing key: only account, transaction, nonce and imported keys other than view keys \
             sign"
        )),
    ))
}

/// [`ensure_signing_key`] for a transaction's `seal_signer` and each of its `other_signers`.
pub(super) fn ensure_transaction_signers<TSpec: WalletSdkSpec>(
    sdk: &WalletSdk<TSpec>,
    seal_signer: &KeyId,
    other_signers: &[KeyId],
) -> Result<(), anyhow::Error> {
    ensure_signing_key(sdk, "seal_signer", seal_signer)?;
    other_signers
        .iter()
        .try_for_each(|key_id| ensure_signing_key(sdk, "other_signers", key_id))
}

pub(super) fn application_error<T: Display>(code: ApplicationErrorCode, details: T) -> anyhow::Error {
    axum_jrpc::error::JsonRpcError::new(
        axum_jrpc::error::JsonRpcErrorReason::ApplicationError(code as i32),
        format!("Application error: '{details}",),
        serde_json::Value::Null,
    )
    .into()
}

fn still_pending(transaction_id: TransactionId) -> anyhow::Error {
    application_error(
        ApplicationErrorCode::TransactionPending,
        format!(
            "Transaction {transaction_id} has not finalized within {}s. The wallet keeps tracking it, and \
             transactions.wait_result reports its outcome.",
            RESULT_WAIT_TIMEOUT.as_secs()
        ),
    )
}

pub(super) fn not_found<T: Display>(details: T) -> anyhow::Error {
    axum_jrpc::error::JsonRpcError::new(
        axum_jrpc::error::JsonRpcErrorReason::ApplicationError(ApplicationErrorCode::NotFound as i32),
        format!("Not found: {details}",),
        serde_json::Value::Null,
    )
    .into()
}

/// Answers a missing thing with the JSON-RPC not-found code rather than as a general error.
///
/// A caller asking for something that does not exist yet - an account it has just created, an input
/// whose creating transaction has not reached the indexer - has to be able to tell that apart from
/// the wallet failing. Only the first is worth retrying, and a general error tells it the opposite.
///
/// The distinction is already carried by [`IsNotFoundError`]; this puts it where the JSON-RPC layer
/// reads it, keeping the error's own message. Named for that layer because that is the whole of what
/// it does: the error it produces means something only to a JSON-RPC caller.
pub(super) trait OrJrpcNotFound<T> {
    fn or_jrpc_not_found(self) -> Result<T, anyhow::Error>;
}

impl<T, E: IsNotFoundError + Into<anyhow::Error>> OrJrpcNotFound<T> for Result<T, E> {
    fn or_jrpc_not_found(self) -> Result<T, anyhow::Error> {
        self.map_err(|e| {
            if e.is_not_found_error() {
                not_found(e.into())
            } else {
                e.into()
            }
        })
    }
}

pub(super) fn invalid_request<T: Display>(details: T) -> anyhow::Error {
    application_error(
        ApplicationErrorCode::InvalidRequest,
        format!("Invalid request: {details}"),
    )
}

/// Seed recovery checks account key indexes from 0, adds an account for each and removes the unused ones when it
/// finishes. An account created while it runs takes one of those indexes.
pub(super) fn ensure_seed_recovery_finished<TSpec: WalletSdkSpec>(sdk: &WalletSdk<TSpec>) -> Result<(), anyhow::Error> {
    if sdk.is_recovery_needed()? {
        return Err(invalid_request(
            "the wallet is still recovering accounts from its seed. Create the account after recovery has finished",
        ));
    }
    Ok(())
}

pub(super) fn unauthorized<T: Display>(details: T) -> anyhow::Error {
    application_error(ApplicationErrorCode::Unauthorized, format!("Unauthorized: {details}"))
}

pub(super) fn transaction_rejected<T: Display>(details: T) -> anyhow::Error {
    application_error(
        ApplicationErrorCode::TransactionRejected,
        format!("Transaction rejected: {details}"),
    )
}

pub(super) fn faucet_already_claimed() -> anyhow::Error {
    application_error(
        ApplicationErrorCode::FaucetAlreadyClaimed,
        "This account has already claimed testnet funds",
    )
}

pub(super) fn faucet_empty() -> anyhow::Error {
    application_error(
        ApplicationErrorCode::FaucetEmpty,
        "The testnet faucet does not hold enough funds for a claim. Claim tTARI by burning on L1 instead",
    )
}

pub(super) fn general_error<T: Display>(details: T) -> anyhow::Error {
    application_error(ApplicationErrorCode::GeneralError, details)
}

/// Reject anything that isn't a single, non-`..` path component, so caller-supplied
/// names cannot escape the burn-proof directory via `Path::join`.
pub(crate) fn validate_burn_proof_file_name(file_name: &str) -> anyhow::Result<()> {
    let p = Path::new(file_name);
    if p.components().count() != 1 || file_name.contains("..") {
        return Err(anyhow!("Invalid file name"));
    }
    Ok(())
}

/// Converts a `CompleteClaimBurnProof` (the L1 burn proof file format) into the
/// `ClaimBurnProofContents` used by the wallet daemon for claim transactions.
pub(crate) fn complete_burn_proof_to_contents(proof: CompleteClaimBurnProof) -> anyhow::Result<ClaimBurnProofContents> {
    let encrypted_data = EncryptedData::try_from(proof.encrypted_data).map_err(|len| {
        anyhow!(
            "Invalid encrypted data length: {}. Expected max length: {}",
            len,
            EncryptedData::MAX_MEMO_SIZE
        )
    })?;
    let claim_proof = claim_proof_from_l1(&proof.claim_proof).map_err(|e| anyhow!("Invalid burn proof: {e}"))?;
    Ok(ClaimBurnProofContents {
        claim_proof,
        encrypted_data,
    })
}

#[derive(Debug, Default)]
pub(crate) struct ComponentUpgradeInspection {
    pub upgradable_components: Vec<UpgradableComponentInfo>,
    pub uninspected_components: Vec<ComponentAddress>,
}

/// Returns `true` when `upgrade_rule` and `owner_rule` can never pass `check_component_upgrade_rule`
/// in the engine (`UpdateRule::Locked`, `UpdateRule::AccessRule(AccessRule::DenyAll)`, or
/// `UpdateRule::Owner` when ownership is `SubstateOwnerRule::None` or
/// `SubstateOwnerRule::ByAccessRule(AccessRule::DenyAll)`).
fn is_effectively_locked(upgrade_rule: &UpdateRule, owner_rule: &SubstateOwnerRule) -> bool {
    matches!(
        (upgrade_rule, owner_rule),
        (UpdateRule::Locked, _) |
            (UpdateRule::AccessRule(AccessRule::DenyAll), _) |
            (
                UpdateRule::Owner,
                SubstateOwnerRule::None | SubstateOwnerRule::ByAccessRule(AccessRule::DenyAll)
            )
    )
}

/// Flags each component referenced by `component_addresses` (or upped in `diff`, if present) whose
/// `upgrade_rule` is not effectively locked, and reports any component whose substate could not be
/// fetched or verified from the network, in the order first referenced.
///
/// Builtin account components known to the wallet (either in the accounts table or cached with
/// [`ACCOUNT_TEMPLATE_ADDRESS`]) are always [`UpdateRule::Locked`] and are skipped without a
/// network round-trip.
pub(crate) async fn find_upgradable_components<TSpec: WalletSdkSpec>(
    sdk: &WalletSdk<TSpec>,
    component_addresses: impl IntoIterator<Item = ComponentAddress>,
    diff: Option<&SubstateDiff>,
) -> Result<ComponentUpgradeInspection, anyhow::Error> {
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for address in component_addresses {
        if seen.insert(address) {
            ordered.push(address);
        }
    }
    if let Some(d) = diff {
        for (id, substate) in d.up_iter() {
            if substate.substate_value().component().is_some() &&
                let Some(address) = id.as_component_address() &&
                seen.insert(address)
            {
                ordered.push(address);
            }
        }
    }

    if ordered.is_empty() {
        return Ok(ComponentUpgradeInspection::default());
    }

    let accounts_api = sdk.accounts_api();
    let substate_api = sdk.substate_api();
    let mut candidates = Vec::new();
    let mut to_fetch = Vec::new();

    for address in ordered {
        if accounts_api.exists_by_address(&address)? {
            continue;
        }
        let substate_id = SubstateId::Component(address);
        if substate_api
            .get_substate(&substate_id)
            .optional()?
            .is_some_and(|record| record.template_address == Some(ACCOUNT_TEMPLATE_ADDRESS))
        {
            continue;
        }
        let in_diff = diff.is_some_and(|d| {
            d.up_iter()
                .any(|(id, substate)| *id == substate_id && substate.substate_value().component().is_some())
        });
        if !in_diff {
            to_fetch.push(substate_id);
        }
        candidates.push(address);
    }

    let fetched = if to_fetch.is_empty() {
        HashMap::new()
    } else {
        match substate_api.get_each_substate_from_network(&to_fetch).await {
            Ok(results) => {
                let mut map = HashMap::with_capacity(results.len());
                for (id, result) in results {
                    match result {
                        Ok(read_substate) => {
                            map.insert(id, read_substate.substate);
                        },
                        Err(err) => {
                            log::warn!(
                                target: "tari::ootle::wallet_daemon::handlers",
                                "Failed to fetch or verify component substate {id} to inspect upgrade rule: {err}"
                            );
                        },
                    }
                }
                map
            },
            Err(err) => {
                log::warn!(
                    target: "tari::ootle::wallet_daemon::handlers",
                    "Failed to fetch component substates to inspect upgrade rules: {err}"
                );
                HashMap::new()
            },
        }
    };

    let mut upgradable_components = Vec::new();
    let mut uninspected_components = Vec::new();
    for component_address in candidates {
        let substate_id = SubstateId::Component(component_address);
        let component = diff
            .and_then(|d| {
                d.up_iter()
                    .find(|(id, _)| *id == substate_id)
                    .and_then(|(_, s)| s.substate_value().component())
            })
            .or_else(|| fetched.get(&substate_id).and_then(|s| s.component()));

        let Some(component) = component else {
            uninspected_components.push(component_address);
            continue;
        };
        if *component.template_address() == ACCOUNT_TEMPLATE_ADDRESS {
            continue;
        }
        if !is_effectively_locked(component.upgrade_rule(), component.owner_rule()) {
            upgradable_components.push(UpgradableComponentInfo {
                component_address,
                template_address: *component.template_address(),
                upgrade_rule: component.upgrade_rule().clone(),
                owner_rule: component.owner_rule().clone(),
            });
        }
    }

    Ok(ComponentUpgradeInspection {
        upgradable_components,
        uninspected_components,
    })
}

#[cfg(test)]
mod tests {
    use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};

    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct TestError(&'static str);

    impl IsNotFoundError for TestError {
        fn is_not_found_error(&self) -> bool {
            self.0 == "missing"
        }
    }

    fn is_not_found_code(e: &anyhow::Error) -> bool {
        matches!(
            e.downcast_ref::<JsonRpcError>().map(|e| e.error_reason()),
            Some(JsonRpcErrorReason::ApplicationError(code)) if code == ApplicationErrorCode::NotFound as i32
        )
    }

    fn error_code(e: &anyhow::Error) -> Option<i32> {
        match e.downcast_ref::<JsonRpcError>().map(|e| e.error_reason()) {
            Some(JsonRpcErrorReason::ApplicationError(code)) => Some(code),
            _ => None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_transaction_that_never_finalizes_is_reported_pending_after_the_timeout() {
        let (_sender, mut events) = broadcast::channel::<WalletEvent>(1);
        let transaction_id = TransactionId::new([1; 32]);

        let err = wait_for_result(&mut events, transaction_id).await.unwrap_err();
        assert_eq!(error_code(&err), Some(ApplicationErrorCode::TransactionPending as i32));

        let account = ComponentAddress::from_array([2; 32]);
        let err = wait_for_result_and_account(&mut events, &transaction_id, &account)
            .await
            .unwrap_err();
        assert_eq!(error_code(&err), Some(ApplicationErrorCode::TransactionPending as i32));
    }

    #[test]
    fn a_not_found_error_keeps_its_message() {
        let err = Result::<(), _>::Err(TestError("missing"))
            .or_jrpc_not_found()
            .unwrap_err();
        assert!(is_not_found_code(&err));
        // The error keeps its own message rather than being flattened to "not found".
        assert!(err.to_string().contains("missing"));
    }

    /// Only a missing resource is retryable, so nothing else may be reported as one.
    #[test]
    fn any_other_failure_is_left_alone() {
        let err = Result::<(), _>::Err(TestError("broken"))
            .or_jrpc_not_found()
            .unwrap_err();
        assert!(!is_not_found_code(&err));
        assert!(err.downcast_ref::<TestError>().is_some());
    }
}
