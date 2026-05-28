//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display};

use anyhow::anyhow;
use ootle_byte_type::ToByteType;
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    confidential::{AbridgedTransactionKernel, EncodedMerkleProof, MinotariBurnClaimProof},
};
use tari_ootle_common_types::{SubstateRequirement, optional::Optional};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    WalletSdk,
    WalletSdkSpec,
    apis::accounts::{AccountsApi, AccountsApiError},
    models::{AccountWithAddress, DerivedKeyIndex, TransactionFinalizedEvent, WalletEvent},
};
use tari_ootle_walletd_client::{ComponentAddressOrName, types::ClaimBurnProofContents};
use tari_sidechain::CompleteClaimBurnProof;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib_types::{ComponentAddress, EncryptedData};
use tokio::sync::broadcast;

use crate::server::ApplicationErrorCode;

pub async fn wait_for_result(
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

pub(super) fn application_error<T: Display>(code: ApplicationErrorCode, details: T) -> anyhow::Error {
    axum_jrpc::error::JsonRpcError::new(
        axum_jrpc::error::JsonRpcErrorReason::ApplicationError(code as i32),
        format!("Application error: '{details}",),
        serde_json::Value::Null,
    )
    .into()
}
pub(super) fn not_found<T: Display>(details: T) -> anyhow::Error {
    axum_jrpc::error::JsonRpcError::new(
        axum_jrpc::error::JsonRpcErrorReason::ApplicationError(ApplicationErrorCode::NotFound as i32),
        format!("Not found: {details}",),
        serde_json::Value::Null,
    )
    .into()
}

pub(super) fn invalid_request<T: Display>(details: T) -> anyhow::Error {
    application_error(
        ApplicationErrorCode::InvalidRequest,
        format!("Invalid request: {details}"),
    )
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

pub(super) fn general_error<T: Display>(details: T) -> anyhow::Error {
    application_error(ApplicationErrorCode::GeneralError, details)
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
    let encoded_merkle_proof = proof
        .claim_proof
        .encoded_merkle_proof
        .encoded_merkle_proof
        .try_into()
        .map_err(|_| anyhow!("Invalid encoded merkle proof length"))?;

    Ok(ClaimBurnProofContents {
        claim_proof: MinotariBurnClaimProof {
            burn_public_key: proof.claim_proof.burn_public_key.to_byte_type(),
            commitment: proof.claim_proof.commitment.to_byte_type(),
            ownership_proof: proof.claim_proof.ownership_proof.to_byte_type(),
            encoded_merkle_proof: EncodedMerkleProof {
                block_hash: proof.claim_proof.encoded_merkle_proof.block_hash.into_array().into(),
                encoded_merkle_proof,
                leaf_index: proof.claim_proof.encoded_merkle_proof.leaf_index,
            },
            kernel: AbridgedTransactionKernel {
                version: proof.claim_proof.kernel.version,
                fee: proof.claim_proof.kernel.fee,
                lock_height: proof.claim_proof.kernel.lock_height,
                excess: proof.claim_proof.kernel.excess.to_byte_type(),
                excess_sig: proof.claim_proof.kernel.excess_sig.to_byte_type(),
            },
            value: proof.claim_proof.value,
            sender_offset_public_key: proof.claim_proof.sender_offset_public_key.to_byte_type(),
        },
        encrypted_data,
    })
}
