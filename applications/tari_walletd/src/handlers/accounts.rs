//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use std::convert::TryFrom;

use anyhow::anyhow;
use axum::headers::authorization::Bearer;
use base64;
use log::*;
use rand::rngs::OsRng;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_engine_types::{
    component::new_component_address_from_public_key,
    confidential::ConfidentialClaim,
    instruction::Instruction,
    substate::{Substate, SubstateId},
    ToByteType,
};
use tari_key_manager::key_manager::DerivedKey;
use tari_ootle_common_types::{optional::Optional, SubstateRequirement};
use tari_ootle_wallet_crypto::ConfidentialProofStatement;
use tari_ootle_wallet_sdk::{
    apis::{confidential_transfer::TransferParams, key_manager, substate::ValidatorScanResult},
    models::NewAccountInfo,
    storage::WalletStore,
    WalletSdk,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    constants::{XTR_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_VAULT_ADDRESS},
    instruction_args,
    models::{Amount, UnclaimedConfidentialOutputAddress},
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, Scalar32Bytes, CONFIDENTIAL_TARI_RESOURCE_ADDRESS},
    types::crypto::CommitmentSignatureBytes,
};
use tari_transaction::args;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        AccountGetDefaultRequest,
        AccountGetRequest,
        AccountGetResponse,
        AccountInfo,
        AccountSetDefaultRequest,
        AccountSetDefaultResponse,
        AccountsCreateFreeTestCoinsRequest,
        AccountsCreateFreeTestCoinsResponse,
        AccountsCreateRequest,
        AccountsCreateResponse,
        AccountsGetBalancesRequest,
        AccountsGetBalancesResponse,
        AccountsListRequest,
        AccountsListResponse,
        AccountsTransferRequest,
        AccountsTransferResponse,
        BalanceEntry,
        ClaimBurnRequest,
        ClaimBurnResponse,
        ConfidentialTransferRequest,
        ConfidentialTransferResponse,
        RevealFundsRequest,
        RevealFundsResponse,
    },
    ComponentAddressOrName,
};
use tokio::task;

use super::context::HandlerContext;
use crate::{
    handlers::helpers::{
        get_account,
        get_account_or_default,
        get_account_with_inputs,
        invalid_params,
        not_found,
        transaction_builder,
        wait_for_result,
        wait_for_result_and_account,
    },
    indexer_jrpc_impl::IndexerJsonRpcNetworkInterface,
    services::TransactionSubmittedEvent,
    DEFAULT_FEE,
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::transaction";

pub async fn handle_create(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsCreateRequest,
) -> Result<AccountsCreateResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let key_manager_api = sdk.key_manager_api();

    if let Some(name) = req.account_name.as_ref() {
        if sdk.accounts_api().get_account_by_name(name).optional()?.is_some() {
            return Err(anyhow!("Account name '{}' already exists", name));
        }
    }

    let default_account = sdk.accounts_api().get_default()?;
    let inputs = sdk
        .substate_api()
        .locate_dependent_substates(&[default_account.address.clone()])
        .await?;

    let signing_key_index = req.key_id.unwrap_or(default_account.key_index);
    let signing_key = key_manager_api.derive_key(key_manager::TRANSACTION_BRANCH, signing_key_index)?;

    let owner_key = key_manager_api.next_key(key_manager::TRANSACTION_BRANCH)?;
    let owner_pk = RistrettoPublicKey::from_secret_key(&owner_key.key).to_byte_type();

    info!(
        target: LOG_TARGET,
        "Creating account with owner token {}. Fees are paid using account '{}' {}",
        owner_pk,
        default_account.name.as_deref().unwrap_or("<None>"),
        default_account.address
    );

    let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);
    let transaction = transaction_builder(context)
        .fee_transaction_pay_from_component(default_account.address.as_component_address().unwrap(), max_fee)
        .create_account(owner_pk)
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .build_and_seal(&signing_key.key);

    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction_with_new_account(transaction, vec![], NewAccountInfo {
            name: req.account_name,
            key_index: owner_key.key_index,
            is_default: req.is_default,
        })
        .await?;

    let event = wait_for_result(&mut events, tx_id).await?;
    if let Some(reason) = event.finalize.any_reject() {
        return Err(anyhow!("Create account transaction failed: {}", reason));
    }

    let address = event
        .finalize
        .result
        .accept()
        .unwrap()
        .up_iter()
        .find(|(_, v)| v.version() == 0 && is_account_substate(v))
        .map(|(a, _)| a.clone())
        .ok_or_else(|| anyhow!("Finalize result did not UP any new version 0 component"))?;

    Ok(AccountsCreateResponse {
        address,
        public_key: owner_pk,
        result: event.finalize,
    })
}

pub async fn handle_set_default(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountSetDefaultRequest,
) -> Result<AccountSetDefaultResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let account = get_account(&req.account, &sdk.accounts_api())?;
    sdk.accounts_api().set_default_account(&account.address)?;
    Ok(AccountSetDefaultResponse {})
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsListRequest,
) -> Result<AccountsListResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let accounts = sdk.accounts_api().get_many(req.offset, req.limit)?;
    let total = sdk.accounts_api().count()?;
    let km = sdk.key_manager_api();
    let accounts = accounts
        .into_iter()
        .map(|a| {
            let key = km.derive_key(key_manager::TRANSACTION_BRANCH, a.key_index)?;
            let pk = RistrettoPublicKey::from_secret_key(&key.key);
            Ok(AccountInfo {
                account: a,
                public_key: pk.to_byte_type(),
            })
        })
        .collect::<Result<_, anyhow::Error>>()?;

    Ok(AccountsListResponse { accounts, total })
}

pub async fn handle_get_balances(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsGetBalancesRequest,
) -> Result<AccountsGetBalancesResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    let account = get_account_or_default(req.account, &sdk.accounts_api())?;
    context.check_auth(token, &[JrpcPermission::AccountBalance(account.clone().address)])?;
    if req.refresh {
        context
            .account_monitor()
            .refresh_account(account.address.clone())
            .await?;
    }
    let vaults = sdk.accounts_api().get_vaults_by_account(&account.address)?;

    let mut balances = Vec::with_capacity(vaults.len());
    for vault in vaults {
        balances.push(BalanceEntry {
            vault_address: vault.address,
            resource_address: vault.resource_address,
            balance: vault.revealed_balance,
            resource_type: vault.resource_type,
            confidential_balance: vault.confidential_balance,
            token_symbol: vault.token_symbol,
        })
    }

    Ok(AccountsGetBalancesResponse {
        address: account.address,
        balances,
    })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountGetRequest,
) -> Result<AccountGetResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let account = get_account(&req.name_or_address, &sdk.accounts_api())?;
    let km = sdk.key_manager_api();
    let key = km.derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?;
    let public_key = RistrettoPublicKey::from_secret_key(&key.key);
    Ok(AccountGetResponse {
        account,
        public_key: public_key.to_byte_type(),
    })
}

pub async fn handle_get_default(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _req: AccountGetDefaultRequest,
) -> Result<AccountGetResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::AccountInfo])?;
    let sdk = context.wallet_sdk();
    let account = get_account_or_default(None, &sdk.accounts_api())?;
    let km = sdk.key_manager_api();
    let key = km.derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?;
    let public_key = RistrettoPublicKey::from_secret_key(&key.key);
    Ok(AccountGetResponse {
        account,
        public_key: public_key.to_byte_type(),
    })
}

#[allow(clippy::too_many_lines)]
pub async fn handle_reveal_funds(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: RevealFundsRequest,
) -> Result<RevealFundsResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk().clone();
    let notifier = context.notifier().clone();
    let transaction_service = context.transaction_service().clone();

    // If the caller aborts the request early, this async block would be aborted at any await point. To avoid this, we
    // spawn a task that will continue running.
    let ctx = context.clone();
    task::spawn(async move {
        let account = get_account_or_default(req.account, &sdk.accounts_api())?;

        let vault = sdk
            .accounts_api()
            .get_vault_by_resource(&account.address, &CONFIDENTIAL_TARI_RESOURCE_ADDRESS)?;

        let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);
        let amount_to_reveal = req.amount_to_reveal + if req.pay_fee_from_reveal { max_fee } else { 0.into() };

        let proof_id = sdk.confidential_outputs_api().add_proof(&vault.address)?;

        let (inputs, input_value) =
            sdk.confidential_outputs_api()
                .lock_outputs_by_amount(&vault.address, amount_to_reveal, proof_id)?;
        let input_amount = Amount::try_from(input_value)?;

        let account_key = sdk
            .key_manager_api()
            .derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?;

        let output_mask = sdk.key_manager_api().next_key(key_manager::TRANSACTION_BRANCH)?;
        let (_, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);

        let remaining_confidential_amount = input_amount - amount_to_reveal;
        let encrypted_data = sdk.confidential_crypto_api().encrypt_value_and_mask(
            remaining_confidential_amount.as_u64_checked().unwrap(),
            &output_mask.key,
            &public_nonce,
            &account_key.key,
        )?;

        let output_statement = ConfidentialProofStatement {
            amount: remaining_confidential_amount,
            mask: output_mask.key,
            sender_public_nonce: public_nonce,
            minimum_value_promise: 0,
            encrypted_data,
            resource_view_key: None,
        };

        let inputs = sdk
            .confidential_outputs_api()
            .resolve_output_masks(inputs, key_manager::TRANSACTION_BRANCH)?;

        let reveal_proof = sdk.confidential_crypto_api().generate_withdraw_proof(
            &inputs,
            Amount::zero(),
            Some(&output_statement),
            amount_to_reveal,
            None,
            Amount::zero(),
        )?;

        info!(
            target: LOG_TARGET,
            "Locked {} inputs ({}) for reveal funds transaction on account: {}",
            inputs.len(),
            input_amount,
            account.address
        );

        let account_address = account.address.as_component_address().unwrap();

        let mut builder = transaction_builder(&ctx);
        if req.pay_fee_from_reveal {
            builder = builder.with_fee_instructions_builder(|builder| {
                builder
                    .call_method(account_address, "withdraw_confidential", args![
                        CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
                        reveal_proof.clone()
                    ])
                    .put_last_instruction_output_on_workspace("revealed")
                    .call_method(account_address, "deposit", args![Workspace("revealed")])
                    .call_method(account_address, "pay_fee", args![max_fee])
            });
        } else {
            builder = builder
                .fee_transaction_pay_from_component(account_address, max_fee)
                .call_method(account_address, "withdraw_confidential", args![
                    CONFIDENTIAL_TARI_RESOURCE_ADDRESS,
                    reveal_proof
                ])
                .put_last_instruction_output_on_workspace("revealed")
                .call_method(account_address, "deposit", args![Workspace("revealed")]);
        }

        // Add the account component
        let account_substate = sdk.substate_api().get_substate(&account.address)?;
        // Add all versioned account child addresses as inputs
        let child_addresses = sdk.substate_api().load_dependent_substates(&[&account.address])?;
        let mut inputs = Vec::with_capacity(child_addresses.len() + 1);
        inputs.push(SubstateRequirement::from(account_substate.substate_id));
        inputs.extend(child_addresses);

        let transaction = builder.with_inputs(inputs).build_and_seal(&account_key.key);

        sdk.confidential_outputs_api()
            .proofs_set_transaction_hash(proof_id, transaction.calculate_id())?;

        let mut events = notifier.subscribe();
        let tx_id = transaction_service.submit_transaction(transaction, vec![]).await?;

        let finalized = wait_for_result(&mut events, tx_id).await?;
        if let Some(reason) = finalized.finalize.fee_reject() {
            return Err(anyhow::anyhow!("Transaction failed: {}", reason));
        }

        Ok(RevealFundsResponse {
            transaction_id: tx_id,
            fee: finalized.final_fee,
            result: finalized.finalize,
        })
    })
    .await?
}

#[allow(clippy::too_many_lines)]
pub async fn handle_claim_burn(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ClaimBurnRequest,
) -> Result<ClaimBurnResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();

    let ClaimBurnRequest {
        account,
        claim_proof,
        max_fee,
        key_id,
    } = req;

    let max_fee = max_fee.unwrap_or(DEFAULT_FEE);
    if max_fee.is_negative() {
        return Err(invalid_params("fee", Some("cannot be negative")));
    }

    let reciprocal_claim_public_key = RistrettoPublicKey::from_canonical_bytes(
        &base64::decode(
            claim_proof["reciprocal_claim_public_key"]
                .as_str()
                .ok_or_else(|| invalid_params::<&str>("reciprocal_claim_public_key", None))?,
        )
        .map_err(|e| invalid_params("reciprocal_claim_public_key", Some(e)))?,
    )
    .map_err(|e| invalid_params("reciprocal_claim_public_key", Some(e)))?;
    let commitment = base64::decode(
        claim_proof["commitment"]
            .as_str()
            .ok_or_else(|| invalid_params::<&str>("commitment", None))?,
    )
    .map_err(|e| invalid_params("commitment", Some(e)))?;
    let range_proof = base64::decode(
        claim_proof["range_proof"]
            .as_str()
            .or_else(|| claim_proof["rangeproof"].as_str())
            .ok_or_else(|| invalid_params::<&str>("range_proof", None))?,
    )
    .map_err(|e| invalid_params("range_proof", Some(e)))?;

    let public_nonce = RistrettoPublicKey::from_canonical_bytes(
        &base64::decode(
            claim_proof["ownership_proof"]["public_nonce"]
                .as_str()
                .ok_or_else(|| invalid_params::<&str>("ownership_proof.public_nonce", None))?,
        )
        .map_err(|e| invalid_params("ownership_proof.public_nonce", Some(e)))?,
    )
    .map_err(|e| invalid_params("ownership_proof.public_nonce", Some(e)))?;
    let u = Scalar32Bytes::from_bytes(
        &base64::decode(
            claim_proof["ownership_proof"]["u"]
                .as_str()
                .ok_or_else(|| invalid_params::<&str>("ownership_proof.u", None))?,
        )
        .map_err(|e| invalid_params("ownership_proof.u", Some(e)))?,
    )
    .map_err(|e| invalid_params("ownership_proof.u", Some(e)))?;
    let v = Scalar32Bytes::from_bytes(
        &base64::decode(
            claim_proof["ownership_proof"]["v"]
                .as_str()
                .ok_or_else(|| invalid_params::<&str>("ownership_proof.v", None))?,
        )
        .map_err(|e| invalid_params("ownership_proof.v", Some(e)))?,
    )
    .map_err(|e| invalid_params("ownership_proof.v", Some(e)))?;

    let mut inputs = vec![];
    let accounts_api = sdk.accounts_api();
    let (account_address, account_secret_key, new_account_name) =
        get_or_create_account(&account, &accounts_api, key_id, sdk, &mut inputs)?;

    let account_public_key = RistrettoPublicKey::from_secret_key(&account_secret_key.key);

    info!(
        target: LOG_TARGET,
        "Signing claim burn with key {}. This must be the same as the claiming key used in the burn transaction.",
        account_public_key
    );

    // Add all versioned account child addresses as inputs
    // add the commitment substate id as input to the claim burn transaction
    let commitment_substate_address =
        SubstateRequirement::unversioned(UnclaimedConfidentialOutputAddress::try_from(commitment.as_slice())?);
    inputs.push(commitment_substate_address.clone());

    info!(
        target: LOG_TARGET,
        "Loaded {} inputs for claim burn transaction on account: {:?}",
        inputs.len(),
        account
    );

    // We have to unmask the commitment to allow us to reveal funds for the fee payment
    let ValidatorScanResult { substate: output, .. } = sdk
        .substate_api()
        .scan_for_substate(
            &commitment_substate_address.substate_id,
            commitment_substate_address.version,
        )
        .await?;
    let output = output.into_unclaimed_confidential_output().ok_or_else(|| {
        anyhow!(
            "Expected the indexer to return an unclaimed confidential output substate for {}, but another substate \
             type was returned",
            commitment_substate_address.substate_id
        )
    })?;
    let unmasked_output = sdk.confidential_crypto_api().unblind_output(
        &output.commitment,
        &output.encrypted_data,
        &account_secret_key.key,
        &reciprocal_claim_public_key,
    )?;

    let mask = sdk.key_manager_api().next_key(key_manager::TRANSACTION_BRANCH)?;
    let (nonce, output_public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);

    let final_amount = Amount::try_from(unmasked_output.value)? - max_fee;
    if final_amount.is_negative() {
        return Err(anyhow::anyhow!(
            "Fee ({}) is greater than the claimed output amount ({})",
            max_fee,
            unmasked_output.value
        ));
    }

    // TODO: validate the proof_of_knowledge from the claim before submitting the transaction

    let encrypted_data = sdk.confidential_crypto_api().encrypt_value_and_mask(
        final_amount.as_u64_checked().unwrap(),
        &mask.key,
        &account_public_key,
        &nonce,
    )?;

    let output_statement = ConfidentialProofStatement {
        amount: final_amount,
        mask: mask.key,
        sender_public_nonce: output_public_nonce,
        minimum_value_promise: 0,
        encrypted_data,
        resource_view_key: None,
    };

    let reveal_proof = sdk.confidential_crypto_api().generate_withdraw_proof(
        &[unmasked_output],
        Amount::zero(),
        Some(&output_statement).filter(|o| !o.amount.is_zero()),
        max_fee,
        None,
        Amount::zero(),
    )?;

    let instructions = vec![Instruction::ClaimBurn {
        claim: Box::new(ConfidentialClaim {
            public_key: reciprocal_claim_public_key.to_byte_type(),
            output_address: commitment_substate_address
                .substate_id
                .as_unclaimed_confidential_output_address()
                .unwrap(),
            range_proof,
            proof_of_knowledge: CommitmentSignatureBytes::new(
                PedersenCommitmentBytes::from_public_key(public_nonce.to_byte_type()),
                u,
                v,
            ),
            withdraw_proof: Some(reveal_proof),
        }),
    }];

    // ------------------------------
    let (tx_id, finalized) = finish_claiming(
        instructions,
        account_address,
        new_account_name,
        sdk,
        inputs,
        account_public_key.to_byte_type(),
        max_fee,
        account_secret_key,
        &accounts_api,
        context,
    )
    .await?;

    Ok(ClaimBurnResponse {
        transaction_id: tx_id,
        fee: finalized.final_fee,
        result: finalized.finalize,
    })
}

async fn finish_claiming<T: WalletStore>(
    fee_instructions: Vec<Instruction>,
    account_address: SubstateId,
    new_account_name: Option<String>,
    sdk: &WalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>,
    mut inputs: Vec<SubstateRequirement>,
    account_public_key: RistrettoPublicKeyBytes,
    max_fee: Amount,
    account_secret_key: DerivedKey<RistrettoPublicKey>,
    accounts_api: &tari_ootle_wallet_sdk::apis::accounts::AccountsApi<'_, T>,
    context: &HandlerContext,
) -> Result<
    (
        tari_transaction::TransactionId,
        crate::services::TransactionFinalizedEvent,
    ),
    anyhow::Error,
> {
    let mut fee_builder = transaction_builder(context)
        .with_instructions(fee_instructions)
        .put_last_instruction_output_on_workspace("bucket");

    let account_component_address = account_address
        .as_component_address()
        .ok_or_else(|| anyhow!("Invalid account address"))?;
    if new_account_name.is_none() {
        // Add all versioned account child addresses as inputs unless the account is new
        let child_addresses = sdk.substate_api().load_dependent_substates(&[&account_address])?;
        inputs.extend(child_addresses);
        fee_builder = fee_builder.call_method(account_component_address, "deposit", args![Workspace("bucket")]);
    } else {
        fee_builder = fee_builder.create_account_with_bucket(account_public_key, "bucket");
    }

    fee_builder = fee_builder.call_method(account_component_address, "pay_fee", args![max_fee]);

    let transaction = transaction_builder(context)
        .with_fee_instructions(fee_builder.build_unsigned_transaction().into_instructions())
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .build_and_seal(&account_secret_key.key);
    let is_first_account = accounts_api.count()? == 0;
    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction_with_opts(
            transaction,
            vec![],
            new_account_name.map(|name| NewAccountInfo {
                name: Some(name),
                key_index: account_secret_key.key_index,
                is_default: is_first_account,
            }),
        )
        .await?;

    // Wait for the monitor to pick up the new or updated account
    let (finalized, _) = wait_for_result_and_account(&mut events, &tx_id, &account_address).await?;
    // let finalized = wait_for_result(&mut events, tx_id).await?;
    if let Some(reject) = finalized.finalize.fee_reject() {
        return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
    }
    if let Some(reason) = finalized.finalize.any_reject() {
        return Err(anyhow::anyhow!(
            "Fee transaction succeeded (fees charged) however the transaction failed: {}",
            reason
        ));
    }

    Ok((tx_id, finalized))
}

/// Mints free test coins into an account. If an account name is provided which does not exist, that account is created
pub async fn handle_create_free_test_coins(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsCreateFreeTestCoinsRequest,
) -> Result<AccountsCreateFreeTestCoinsResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();

    let AccountsCreateFreeTestCoinsRequest {
        account,
        amount,
        max_fee,
        key_id,
    } = req;

    let max_fee = max_fee.unwrap_or(DEFAULT_FEE);
    if max_fee.is_negative() {
        return Err(invalid_params("fee", Some("cannot be negative")));
    }

    let mut inputs = vec![
        SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
        SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
    ];
    let accounts_api = sdk.accounts_api();
    let (account_address, account_secret_key, new_account_name) =
        get_or_create_account(&account, &accounts_api, key_id, sdk, &mut inputs)?;

    let account_public_key = RistrettoPublicKey::from_secret_key(&account_secret_key.key).to_byte_type();

    let instructions = vec![Instruction::CallMethod {
        call: XTR_FAUCET_COMPONENT_ADDRESS.into(),
        method: "take".to_string(),
        args: instruction_args![amount],
    }];

    // ------------------------------
    let (tx_id, finalized) = finish_claiming(
        instructions,
        account_address.clone(),
        new_account_name,
        sdk,
        inputs,
        account_public_key,
        max_fee,
        account_secret_key,
        &accounts_api,
        context,
    )
    .await?;

    let account = accounts_api.get_account_by_address(&account_address)?;

    Ok(AccountsCreateFreeTestCoinsResponse {
        account,
        transaction_id: tx_id,
        amount,
        fee: max_fee,
        result: finalized.finalize,
        public_key: account_public_key,
    })
}

fn get_or_create_account<T: WalletStore>(
    account: &Option<ComponentAddressOrName>,
    accounts_api: &tari_ootle_wallet_sdk::apis::accounts::AccountsApi<'_, T>,
    key_id: Option<u64>,
    sdk: &WalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>,
    inputs: &mut Vec<SubstateRequirement>,
) -> Result<(SubstateId, DerivedKey<RistrettoPublicKey>, Option<String>), anyhow::Error> {
    let maybe_account = match account {
        Some(ref addr_or_name) => get_account(addr_or_name, accounts_api).optional()?,
        None => {
            let account = accounts_api
                .get_default()
                .optional()?
                .ok_or_else(|| not_found("No default account found. Please create or set a default account."))?;

            Some(account)
        },
    };
    let (account_address, account_secret_key, new_account_name) = match maybe_account {
        Some(account) => {
            let account_secret_key = sdk
                .key_manager_api()
                .derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?;
            let account_substate = sdk.substate_api().get_substate(&account.address)?;
            inputs.push(account_substate.substate_id.into());

            (account.address, account_secret_key, None)
        },
        None => {
            let name = account.as_ref().unwrap().name().ok_or_else(|| {
                invalid_params(
                    "account.Name",
                    Some("Account name must be provided when creating a new account"),
                )
            })?;
            let account_secret_key = key_id
                .map(|idx| sdk.key_manager_api().derive_key(key_manager::TRANSACTION_BRANCH, idx))
                .unwrap_or_else(|| sdk.key_manager_api().next_key(key_manager::TRANSACTION_BRANCH))?;
            let account_pk = RistrettoPublicKey::from_secret_key(&account_secret_key.key);

            let account_address =
                new_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &account_pk.to_byte_type());

            // We have no involved substate addresses, so we need to add an output
            (account_address.into(), account_secret_key, Some(name.to_string()))
        },
    };
    Ok((account_address, account_secret_key, new_account_name))
}

#[allow(clippy::too_many_lines)]
pub async fn handle_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsTransferRequest,
) -> Result<AccountsTransferResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk().clone();

    let (account, mut inputs) = get_account_with_inputs(req.account, &sdk)?;

    // get the source account component address
    let source_account_address = account
        .address
        .as_component_address()
        .ok_or_else(|| anyhow!("Invalid account address"))?;

    // add the input for the source account vault substate
    let src_vault = sdk
        .accounts_api()
        .get_vault_by_resource(&account.address, &req.resource_address)?;
    let src_vault_substate = sdk.substate_api().get_substate(&src_vault.address)?;
    inputs.insert(src_vault_substate.substate_id.into());

    let resource_substate_address = SubstateRequirement::unversioned(src_vault.resource_address);
    inputs.insert(resource_substate_address.clone());

    let destination_account_address =
        new_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &req.destination_public_key);
    let existing_dest_account = sdk
        .substate_api()
        .scan_for_substate(&SubstateId::Component(destination_account_address), None)
        .await
        .optional()?;

    let mut builder = transaction_builder(context);

    if let Some(ValidatorScanResult { address, substate }) = existing_dest_account {
        inputs.insert(address.into());

        // Figure out which vault to add as an input
        let Some(component) = substate.component() else {
            return Err(anyhow::anyhow!(
                "The destination account {} is not a component. This is unexpected.",
                destination_account_address
            ));
        };
        let indexed = component.body.to_indexed_well_known_types()?;

        let mut found_dest_vault = None;
        for vault_id in indexed.vault_ids() {
            // Local vault?
            match sdk.accounts_api().get_vault(vault_id).optional()? {
                Some(vault) => {
                    if vault.resource_address != src_vault.resource_address {
                        // Continue searching for a vault for the resource address
                        continue;
                    }
                    // Found it - we're sending to our own vault
                    found_dest_vault = Some(*vault_id);
                    break;
                },
                None => {
                    // TODO(perf): slow with lots of vaults
                    let vault = sdk
                        .substate_api()
                        .scan_for_substate(&SubstateId::Vault(*vault_id), None)
                        .await
                        .optional()?;

                    let Some(vault) = vault.and_then(|scan| scan.substate.into_vault()) else {
                        warn!(
                            target: LOG_TARGET,
                            "❓️ The destination account {destination_account_address} contains a vault {vault_id} that was not found. This is unexpected.",
                        );
                        continue;
                    };

                    if *vault.resource_address() != src_vault.resource_address {
                        // Continue searching for a vault for the resource address
                        continue;
                    }

                    // Found it
                    found_dest_vault = Some(*vault_id);
                },
            }
        }

        if let Some(found) = found_dest_vault {
            inputs.insert(SubstateRequirement::unversioned(found));
        }
    } else {
        builder = builder.create_account(req.destination_public_key);
    }

    // build the transaction
    let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);
    let account_secret_key = sdk
        .key_manager_api()
        .derive_key(key_manager::TRANSACTION_BRANCH, account.key_index)?;

    let transaction = builder
        .with_dry_run(req.dry_run)
        .fee_transaction_pay_from_component(source_account_address, max_fee)
        .then(|builder| {
            if let Some(ref badge) = req.proof_from_badge_resource {
                // If we are creating a proof for a badge resource, we need to create the proof first
                builder
                    .call_method(source_account_address, "create_proof_for_resource", args![badge])
                    .put_last_instruction_output_on_workspace("proof")
            } else {
                builder
            }
        })
        .call_method(source_account_address, "withdraw", args![
            req.resource_address,
            req.amount
        ])
        .put_last_instruction_output_on_workspace("bucket")
        .call_method(destination_account_address, "deposit", args![Workspace("bucket")])
        .then(|builder| {
            if req.proof_from_badge_resource.is_some() {
                builder.drop_all_proofs_in_workspace()
            } else {
                builder
            }
        })
        .with_inputs(inputs.into_iter().map(|req| req.into_unversioned()))
        .build_and_seal(&account_secret_key.key);

    // If dry run we can return the result immediately
    if req.dry_run {
        let transaction_id = transaction.calculate_id();
        let execute_result = context
            .transaction_service()
            .submit_dry_run_transaction(transaction, vec![])
            .await?;
        let finalize = execute_result.finalize;
        return Ok(AccountsTransferResponse {
            transaction_id,
            fee: finalize.fee_receipt.total_fees_paid,
            fee_refunded: finalize.fee_receipt.total_fee_payment - finalize.fee_receipt.total_fees_paid,
            result: finalize,
        });
    }

    // Otherwise submit and wait for a result
    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction(transaction, vec![])
        .await?;

    let finalized = wait_for_result(&mut events, tx_id).await?;

    if let Some(reject) = finalized.finalize.result.fee_reject() {
        return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
    }
    if let Some(reason) = finalized.finalize.any_reject() {
        return Err(anyhow::anyhow!(
            "Fee transaction succeeded (fees charged) however the transaction failed: {}",
            reason
        ));
    }
    info!(
        target: LOG_TARGET,
        "✅ Transfer transaction {} finalized. Fee: {}",
        finalized.transaction_id,
        finalized.final_fee
    );

    Ok(AccountsTransferResponse {
        transaction_id: tx_id,
        fee: finalized.final_fee,
        fee_refunded: max_fee - finalized.final_fee,
        result: finalized.finalize,
    })
}

pub async fn handle_confidential_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: ConfidentialTransferRequest,
) -> Result<ConfidentialTransferResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk().clone();
    let notifier = context.notifier().clone();

    if req.amount.is_negative() {
        return Err(invalid_params("amount", Some("must be positive")));
    }
    let transaction_service = context.transaction_service().clone();

    // Spawn here is to prevent the async block from being aborted if the caller aborts the request early as this can
    // cause funds to remain locked indefinitely.
    task::spawn(async move {
        let account = get_account_or_default(req.account, &sdk.accounts_api())?;

        let transfer = sdk
            .confidential_transfer_api()
            .transfer(TransferParams {
                from_account: account.address.as_component_address().unwrap(),
                input_selection: req.input_selection,
                amount: req.amount,
                destination_public_key: req.destination_public_key,
                resource_address: req.resource_address,
                max_fee: req.max_fee.unwrap_or(DEFAULT_FEE),
                output_to_revealed: req.output_to_revealed,
                proof_from_resource: req.proof_from_badge_resource,
                is_dry_run: req.dry_run,
            })
            .await?;

        if req.dry_run {
            let transaction_id = transfer.transaction.calculate_id();
            let exec_result = transaction_service
                .submit_dry_run_transaction(transfer.transaction, transfer.autofill_inputs)
                .await?;
            let finalize = exec_result.finalize;
            return Ok(ConfidentialTransferResponse {
                transaction_id,
                fee: finalize.fee_receipt.total_fees_paid,
                result: finalize,
            });
        }

        let mut events = notifier.subscribe();
        let tx_id = transaction_service
            .submit_transaction(transfer.transaction, transfer.autofill_inputs)
            .await?;

        notifier.notify(TransactionSubmittedEvent {
            transaction_id: tx_id,
            new_account: None,
        });

        let finalized = wait_for_result(&mut events, tx_id).await?;
        if let Some(reject) = finalized.finalize.result.fee_reject() {
            return Err(anyhow::anyhow!("Fee transaction rejected: {}", reject));
        }
        if let Some(reason) = finalized.finalize.fee_reject() {
            return Err(anyhow::anyhow!(
                "Fee transaction succeeded (fees charged) however the transaction failed: {}",
                reason
            ));
        }

        Ok(ConfidentialTransferResponse {
            transaction_id: tx_id,
            fee: finalized.final_fee,
            result: finalized.finalize,
        })
    })
    .await?
}

fn is_account_substate(substate: &Substate) -> bool {
    substate
        .substate_value()
        .component()
        .filter(|c| c.template_address == ACCOUNT_TEMPLATE_ADDRESS)
        .is_some()
}
