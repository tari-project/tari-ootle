//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{array, collections::HashSet};

use anyhow::anyhow;
use axum::headers::authorization::Bearer;
use indexmap::IndexMap;
use log::*;
use rand::rngs::OsRng;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    confidential::TariStealthClaim,
    substate::SubstateId,
    FromByteType,
    ToByteType,
};
use tari_ootle_common_types::{optional::Optional, SubstateRequirement};
use tari_ootle_wallet_crypto::{
    UnblindedOutputStatement,
    UnblindedStealthInputStatement,
    UnblindedStealthOutputStatement,
};
use tari_ootle_wallet_sdk::{
    apis::{
        confidential_transfer::ConfidentialTransferParams,
        key_manager::KeyBranch,
        stealth_transfer::StealthTransferParams,
        substate::ValidatorScanResult,
    },
    models::NewAccountData,
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    constants::{STEALTH_TARI_RESOURCE_ADDRESS, XTR_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_VAULT_ADDRESS},
    models::UnclaimedConfidentialOutputAddress,
    prelude::ResourceType,
    types::Amount,
};
use tari_transaction::args;
use tari_wallet_daemon_client::{
    permissions::JrpcPermission,
    types::{
        AccountGetByKeyIndexRequest,
        AccountGetDefaultRequest,
        AccountGetRequest,
        AccountGetResponse,
        AccountInfo,
        AccountSetDefaultRequest,
        AccountSetDefaultResponse,
        AccountsAssociateStealthResourceRequest,
        AccountsAssociateStealthResourceResponse,
        AccountsCreateFreeTestCoinsRequest,
        AccountsCreateFreeTestCoinsResponse,
        AccountsCreateOrGetRequest,
        AccountsCreateOrGetResponse,
        AccountsCreateRequest,
        AccountsCreateResponse,
        AccountsGetBalancesRequest,
        AccountsGetBalancesResponse,
        AccountsListRequest,
        AccountsListResponse,
        AccountsTransferRequest,
        AccountsTransferResponse,
        BalanceEntry,
        ClaimBurnProof,
        ClaimBurnRequest,
        ClaimBurnResponse,
        ConfidentialTransferRequest,
        ConfidentialTransferResponse,
        ExtClaimBurnProof,
        RevealFundsRequest,
        RevealFundsResponse,
        StealthTransferRequest,
        StealthTransferResponse,
    },
    ComponentAddressOrName,
};
use tokio::task;

use super::context::HandlerContext;
use crate::{
    handlers::helpers::{
        application_error,
        general_error,
        get_account,
        get_account_by_key_index,
        get_account_or_default,
        get_account_with_inputs,
        invalid_params,
        invalid_request,
        not_found,
        transaction_rejected,
        wait_for_result,
        wait_for_result_and_account,
    },
    jrpc_server::ApplicationErrorCode,
    services::TransactionSubmittedEvent,
    DEFAULT_FEE,
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::transaction";

pub async fn handle_create(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsCreateRequest,
) -> Result<AccountsCreateResponse, anyhow::Error> {
    // TODO: fine-grain permissions
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let accounts_api = sdk.accounts_api();

    let set_as_default = req
        .is_default
        .map(Ok)
        .unwrap_or_else(|| accounts_api.any_accounts_exist().map(|b| !b))?;

    let acc = accounts_api
        .create_account(req.account_name.as_deref(), set_as_default, req.key_id)
        .map_err(|e| {
            if e.is_name_exists_error() {
                invalid_request(e)
            } else {
                general_error(e)
            }
        })?;

    info!(
        target: LOG_TARGET,
        "Created account: {acc}."
    );

    Ok(AccountsCreateResponse {
        account: acc.account,
        public_key: acc.owner_public_key,
    })
}

pub async fn handle_create_or_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsCreateOrGetRequest,
) -> Result<AccountsCreateOrGetResponse, anyhow::Error> {
    // TODO: fine-grain permissions
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let accounts_api = sdk.accounts_api();

    let existing_account = match req.account {
        Some(ComponentAddressOrName::ComponentAddress(addr)) => {
            // In this case, we error if a specific address is specified
            let account = accounts_api
                .get_account_by_address(&addr)
                .optional()?
                .ok_or_else(|| not_found(format!("Account with address {addr} not found")))?;
            Some(account)
        },
        // If we cannot find an account with this name, we'll create one
        Some(ComponentAddressOrName::Name(ref name)) => accounts_api.get_account_by_name(name).optional()?,
        // If we cannot find an account with this key index, we'll create one
        None => req
            .key_id
            .map(|id| get_account_by_key_index(sdk, id).optional())
            .transpose()?
            .flatten(),
    };

    if let Some(account) = existing_account {
        info!(
            target: LOG_TARGET,
            "Account already exists: {account}."
        );
        return Ok(AccountsCreateOrGetResponse {
            account: account.account,
            public_key: account.owner_public_key,
            created: false,
        });
    }

    let set_as_default = req
        .is_default
        .map(Ok)
        .unwrap_or_else(|| accounts_api.any_accounts_exist().map(|b| !b))?;

    let acc = accounts_api
        .create_account(req.account.as_ref().and_then(|a| a.name()), set_as_default, req.key_id)
        .map_err(|e| {
            if e.is_name_exists_error() {
                invalid_request(e)
            } else {
                general_error(e)
            }
        })?;

    info!(
        target: LOG_TARGET,
        "Created account: {acc}."
    );

    Ok(AccountsCreateOrGetResponse {
        account: acc.account,
        public_key: acc.owner_public_key,
        created: true,
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
    sdk.accounts_api().set_default_account(account.address())?;
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
            let key = km.derive_account_key(a.key_index)?;
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
    let account = get_account_or_default(req.account.as_ref(), &sdk.accounts_api())?;
    context.check_auth(token, &[JrpcPermission::AccountBalance(account.account.address.into())])?;
    if req.refresh {
        context.account_monitor().refresh_account(*account.address()).await?;
    }
    let vaults = sdk.accounts_api().get_vaults_by_account(account.address())?;
    let stealth_outputs = sdk
        .stealth_outputs_api()
        .get_unspent_outputs_by_account(account.address())?;

    let mut balances = Vec::with_capacity(vaults.len());
    let mut vaulted_resources = HashSet::new();
    for vault in vaults {
        let confidential_balance = if vault.resource_type.is_stealth() {
            let stealth_balance = stealth_outputs
                .iter()
                .filter(|o| o.owner_account == *account.address() && o.resource_address == vault.resource_address)
                .map(|o| o.value)
                .sum::<Amount>();

            if stealth_balance.is_positive() {
                // If the vault has a confidential balance, we don't want to add it to the balances list
                // as it is already included in the vault's revealed balance.
                vaulted_resources.insert(vault.resource_address);
            }
            stealth_balance
        } else {
            vault.confidential_balance
        };

        balances.push(BalanceEntry {
            vault_address: Some(vault.id),
            resource_address: vault.resource_address,
            balance: vault.revealed_balance,
            resource_type: vault.resource_type,
            confidential_balance,
            token_symbol: vault.token_symbol,
            divisibility: vault.divisibility,
        })
    }

    let stealth_outputs = stealth_outputs
        .into_iter()
        .filter(|o| !vaulted_resources.contains(&o.resource_address))
        // NOTE: indexemap used to ensure a consistent order (HashMap causes UI to randomly switch positions for multiple stealth resources)
        .fold(IndexMap::new(), |mut acc, o| {
            acc.entry(o.resource_address)
                .and_modify(|v| *v += o.value)
                .or_insert(o.value);
            acc
        });

    let all_resources = sdk.resources_api().get_many(stealth_outputs.keys())?;

    for (resource_address, total_value) in stealth_outputs {
        let resource = all_resources.get(&resource_address);
        balances.push(BalanceEntry {
            vault_address: None,
            resource_address,
            balance: Amount::zero(),
            resource_type: ResourceType::Stealth,
            confidential_balance: total_value,
            // It's not guaranteed by the wallet that we know the resource, so instead of erroring, we'll return
            // something
            token_symbol: resource.as_ref().and_then(|r| r.token_symbol()).map(|s| s.to_owned()),
            divisibility: resource.as_ref().map(|r| r.divisibility()).unwrap_or(0),
        });
    }

    Ok(AccountsGetBalancesResponse {
        address: *account.address(),
        balances,
    })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountGetRequest,
) -> Result<AccountGetResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::AccountInfo])?;
    let sdk = context.wallet_sdk();
    let account = get_account(&req.name_or_address, &sdk.accounts_api())?;
    Ok(AccountGetResponse {
        account: account.account,
        public_key: account.owner_public_key,
    })
}

pub async fn handle_get_by_key_index(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountGetByKeyIndexRequest,
) -> Result<AccountGetResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::AccountInfo])?;
    let sdk = context.wallet_sdk();
    let account = get_account_by_key_index(sdk, req.key_index).optional()?;
    let account = account.ok_or_else(|| not_found(format!("Account with key index {} not found", req.key_index)))?;
    Ok(AccountGetResponse {
        account: account.account,
        public_key: account.owner_public_key,
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
    Ok(AccountGetResponse {
        account: account.account,
        public_key: account.owner_public_key,
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
        let account = get_account_or_default(req.account.as_ref(), &sdk.accounts_api())?;

        let vault = sdk
            .accounts_api()
            .get_vault_by_resource(account.address(), &STEALTH_TARI_RESOURCE_ADDRESS)?;

        let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);
        let amount_to_reveal = req.amount_to_reveal +
            if req.pay_fee_from_reveal {
                max_fee.into()
            } else {
                0.into()
            };

        let proof_id = sdk.confidential_outputs_api().add_output_lock(&vault.id)?;

        let (inputs, input_amount) =
            sdk.confidential_outputs_api()
                .lock_outputs_by_amount(proof_id, &vault.id, amount_to_reveal)?;

        let account_key = sdk.key_manager_api().derive_account_key(account.key_index())?;

        let output_mask = sdk.key_manager_api().next_key(KeyBranch::ConfidentialMasks)?;
        let (_, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);

        let remaining_confidential_amount = input_amount - amount_to_reveal;
        let encrypted_data = sdk.confidential_crypto_api().encrypt_value_and_mask(
            remaining_confidential_amount.to_u64_checked().unwrap(),
            &output_mask.key,
            &public_nonce,
            &account_key.key,
        )?;

        let output_statement = UnblindedOutputStatement {
            amount: remaining_confidential_amount,
            mask: output_mask.key,
            sender_public_nonce: public_nonce,
            minimum_value_promise: 0,
            encrypted_data,
            resource_view_key: None,
        };

        let inputs = sdk.confidential_outputs_api().resolve_output_masks(inputs)?;

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
            account,
        );

        let account_address = *account.address();

        let mut builder = ctx.transaction_builder();
        if req.pay_fee_from_reveal {
            builder = builder.with_fee_instructions_builder(|builder| {
                builder
                    .call_method(account_address, "withdraw_confidential", args![
                        STEALTH_TARI_RESOURCE_ADDRESS,
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
                    STEALTH_TARI_RESOURCE_ADDRESS,
                    reveal_proof
                ])
                .put_last_instruction_output_on_workspace("revealed")
                .call_method(account_address, "deposit", args![Workspace("revealed")]);
        }

        // Add the account component
        let account_substate = sdk.substate_api().get_substate(&account.account.address.into())?;
        // Add all versioned account child addresses as inputs
        let child_addresses = sdk
            .substate_api()
            .load_dependent_substates(&[&account.account.address.into()])?;
        let mut inputs = Vec::with_capacity(child_addresses.len() + 1);
        inputs.push(SubstateRequirement::from(account_substate.substate_id));
        inputs.extend(child_addresses);

        let transaction = builder.with_inputs(inputs).build_and_seal(&account_key.key);

        sdk.confidential_outputs_api()
            .locks_set_transaction_hash(proof_id, transaction.calculate_id())?;

        let mut events = notifier.subscribe();
        let tx_id = transaction_service.submit_transaction(transaction).await?;

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
    } = req;

    let max_fee = max_fee.unwrap_or(DEFAULT_FEE);

    let ExtClaimBurnProof {
        owner_nonce_key_index,
        claim_proof:
            ClaimBurnProof {
                reciprocal_claim_public_key,
                commitment,
                ownership_proof,
                range_proof,
            },
    } = claim_proof;

    let accounts_api = sdk.accounts_api();
    let account = get_account(&account, &accounts_api)?;
    let account_secret_key = sdk.key_manager_api().derive_account_key(account.key_index())?;

    let network = sdk.config_api().get_network()?;
    let claim_nonce_keypair = sdk
        .key_manager_api()
        .derive_keypair(KeyBranch::Nonce, owner_nonce_key_index)?;

    if !sdk.stealth_crypto_api().validate_burn_claim_ownership_proof(
        network,
        &ownership_proof,
        &commitment,
        &claim_nonce_keypair.public_key.to_byte_type(),
    ) {
        return Err(invalid_params(
            "claim_proof.ownership_proof",
            Some("ownership proof validation failed"),
        ));
    }

    let mut inputs = vec![];

    info!(
        target: LOG_TARGET,
        "ℹ️ Signing claim burn with key {}. NOTE: This must be the same as the claiming key used in the burn transaction for this to succeed.",
        account.owner_public_key
    );

    // Add all versioned account child addresses as inputs
    // add the commitment substate id as input to the claim burn transaction
    let address = UnclaimedConfidentialOutputAddress::from_commitment(&commitment);
    inputs.push(SubstateRequirement::unversioned(address));

    info!(
        target: LOG_TARGET,
        "Loaded {} inputs for claim burn transaction on account: {:?}",
        inputs.len(),
        account
    );

    // We have to unmask the commitment to allow us to reveal funds for the fee payment
    let ValidatorScanResult { substate: output, .. } = sdk
        .substate_api()
        .fetch_substate_from_network(&address.into(), None)
        .await?;
    let output = output.into_unclaimed_confidential_output().ok_or_else(|| {
        anyhow!(
            "Expected the indexer to return an unclaimed confidential output substate for {}, but another substate \
             type was returned",
            address,
        )
    })?;

    let reciprocal_claim_public_key_expanded = RistrettoPublicKey::try_from_byte_type(&reciprocal_claim_public_key)
        .map_err(|e| invalid_params("claim_proof.reciprocal_claim_public_key", Some(e)))?;
    let unmasked_output = sdk.stealth_crypto_api().unblind_output(
        &output.commitment,
        &output.encrypted_data,
        claim_nonce_keypair.secret_key(),
        &reciprocal_claim_public_key_expanded,
    )?;

    let mask = sdk.key_manager_api().next_key(KeyBranch::ConfidentialMasks)?;

    let final_amount = unmasked_output
        .value
        .checked_sub_positive(max_fee.into())
        .ok_or_else(|| invalid_params("max_fee", Some("more fees paid than claimed amount")))?;

    if final_amount.is_zero() {
        return Err(invalid_params("max_fee", Some("fee equals or exceeds claimed amount")));
    }

    let final_amount_u64 = final_amount.to_u64_checked().ok_or_else(|| {
        // NOTE: this can never be anywhere close to this large because this would be more than the total supply of XTM
        // for thousands of years
        application_error(
            ApplicationErrorCode::NotImplemented,
            format!("Amount to spend {final_amount} is too large and not currently supported"),
        )
    })?;

    let (nonce, output_public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let account_owner_public_key = RistrettoPublicKey::from_secret_key(&account_secret_key.key);
    // NOTE: the confidential encryption format and the bullet proofs currently do not support amounts larger than
    // u64::MAX. Apart from it being insane/basically impossible to have that much XTR in a single UTXO, the L1 emission
    // will reach this much in many thousands of years.
    let encrypted_data = sdk.stealth_crypto_api().encrypt_value_and_mask(
        final_amount_u64,
        &mask.key,
        &account_owner_public_key,
        &nonce,
    )?;

    let tag = sdk
        .stealth_crypto_api()
        .derive_stealth_output_tag(network, &account.owner_public_key);

    // Create stealth address - used during spend time
    let stealth_output_owner_public_key =
        sdk.stealth_crypto_api()
            .derive_stealth_owner_public_key(network, &account_owner_public_key, &nonce);

    let output_statement = UnblindedStealthOutputStatement {
        statement: UnblindedOutputStatement {
            amount: final_amount,
            mask: mask.key,
            sender_public_nonce: output_public_nonce.clone(),
            minimum_value_promise: 0,
            encrypted_data,
            resource_view_key: None,
        },
        output_owner_public_key: stealth_output_owner_public_key,
        tag,
    };

    // Generate the correct secret to spend the claimed output
    let input = UnblindedStealthInputStatement {
        mask_and_value: unmasked_output,
        owner_secret: claim_nonce_keypair.secret_key().clone(),
        public_nonce: reciprocal_claim_public_key_expanded,
    };

    let pay_fee_and_mint_output = sdk.stealth_crypto_api().generate_transfer_statement(
        array::from_ref(&input),
        0,
        array::from_ref(&output_statement),
        max_fee,
    )?;

    let transaction = context
        .transaction_builder()
        .with_fee_instructions_builder(|fee_builder| {
            fee_builder
                .claim_burn(TariStealthClaim {
                    burn_public_key: reciprocal_claim_public_key,
                    output_address: address,
                    range_proof,
                    proof_of_knowledge: ownership_proof,
                    transfer_statement: None,
                })
                .pay_fee_stealth(pay_fee_and_mint_output)
        })
        .with_inputs(inputs)
        .build_and_seal(claim_nonce_keypair.secret_key());

    let mut events = context.notifier().subscribe();
    let tx_id = context.transaction_service().submit_transaction(transaction).await?;

    // Wait for the monitor to pick up the new or updated account
    let finalized = wait_for_result(&mut events, tx_id).await?;
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

    Ok(ClaimBurnResponse {
        transaction_id: tx_id,
        fee: finalized.final_fee,
        result: finalized.finalize,
    })
}

/// Takes tXTR from the testnet faucet and deposits them into an existing account.
#[allow(clippy::too_many_lines)]
pub async fn handle_create_free_test_coins(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsCreateFreeTestCoinsRequest,
) -> Result<AccountsCreateFreeTestCoinsResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let accounts_api = sdk.accounts_api();

    let AccountsCreateFreeTestCoinsRequest {
        account,
        amount,
        max_fee,
    } = req;

    let max_fee = max_fee.unwrap_or(DEFAULT_FEE);

    let account = get_account(&account, &accounts_api)
        .optional()?
        .ok_or_else(|| not_found(format!("Account with name or address '{}' not found", account,)))?;

    info!(
        target: LOG_TARGET,
        "💰️ Creating free test coins for account: {} with amount: {} and max fee: {}",
        account.account.address,
        amount,
        max_fee
    );

    let mut inputs = vec![
        SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
        SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
    ];

    if account.is_confirmed_on_chain() {
        info!(
            target: LOG_TARGET,
            "💰️ create free test coins: Account {} is on-chain",
            account.account.address
        );
        // Add account inputs
        let account_substate = sdk.substate_api().get_substate(&account.account.address.into())?;
        inputs.push(account_substate.substate_id.into());

        // Add all versioned account child addresses as inputs
        let child_addresses = sdk
            .substate_api()
            .load_dependent_substates(&[&account.account.address.into()])?;
        info!(
            target: LOG_TARGET,
            "💰️ create free test coins: Loaded {} vaults for existing account: {}",
            child_addresses.len(),
            account
        );
        inputs.extend(child_addresses);
    } else {
        info!(
            target: LOG_TARGET,
            "💰️ create free test coins: Account {} is not on-chain, Will create it",
            account.account.address
        );
    }

    let (account_secret_key, account_public_key) = sdk.key_manager_api().derive_account_keypair(account.key_index())?;

    let transaction = context
        .transaction_builder()
        .with_fee_instructions_builder(|fee_builder| {
            fee_builder
                .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![amount])
                .put_last_instruction_output_on_workspace("faucet_funds")
                .then(|builder| {
                    if account.is_confirmed_on_chain() {
                        builder.call_method(*account.address(), "deposit", args![Workspace("faucet_funds")])
                    } else {
                        // If the account is not on-chain yet, we create it
                        builder.create_account_with_bucket(account_public_key.to_byte_type(), "faucet_funds")
                    }
                })
                .call_method(*account.address(), "pay_fee", args![Amount(max_fee)])
        })
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .build_and_seal(&account_secret_key.key);

    info!(
        target: LOG_TARGET,
        "💰️ create free test coins: Submitting transaction {} for account: {}",
        transaction.calculate_id(),
        account.account,
    );

    let mut events = context.notifier().subscribe();
    let tx_id = context
        .transaction_service()
        .submit_transaction_with_opts(
            transaction,
            account.is_confirmed_on_chain().then(|| NewAccountData {
                address: *account.address(),
            }),
        )
        .await?;

    // Wait for the monitor to pick up the new or updated account
    let (finalized, _) = wait_for_result_and_account(&mut events, &tx_id, account.address()).await?;
    if let Some(reject) = finalized.finalize.fee_reject() {
        return Err(transaction_rejected(format!("Fee transaction rejected: {}", reject)));
    }
    if let Some(reason) = finalized.finalize.any_reject() {
        return Err(anyhow::anyhow!(
            "Fee transaction succeeded (fees charged) however, the transaction failed: {}",
            reason
        ));
    }

    info!(
        target: LOG_TARGET,
        "💰️ create free test coins: Transaction {} finalized for account: {}",
        tx_id,
        account.account,
    );

    // Refresh the account
    let account = accounts_api
        .get_account_by_address(account.address())
        .optional()?
        .ok_or_else(|| not_found(format!("Account with address '{}' not found", account.address())))?;

    Ok(AccountsCreateFreeTestCoinsResponse {
        account: account.account,
        transaction_id: tx_id,
        amount,
        fee: max_fee,
        result: finalized.finalize,
        public_key: account.owner_public_key,
    })
}

#[allow(clippy::too_many_lines)]
pub async fn handle_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsTransferRequest,
) -> Result<AccountsTransferResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk().clone();

    let (account, mut inputs) = get_account_with_inputs(req.account.as_ref(), &sdk)?;

    // get the source account component address
    let source_account_address = *account.address();

    // add the input for the source account vault substate
    let src_vault = sdk
        .accounts_api()
        .get_vault_by_resource(&source_account_address, &req.resource_address)?;
    let src_vault_substate = sdk.substate_api().get_substate(&src_vault.id.into())?;
    inputs.insert(src_vault_substate.substate_id.into());

    let resource_substate_address = SubstateRequirement::unversioned(src_vault.resource_address);
    inputs.insert(resource_substate_address.clone());

    let destination_account_address =
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &req.destination_public_key);
    let existing_dest_account = sdk
        .substate_api()
        .fetch_substate_from_network(&SubstateId::Component(destination_account_address), None)
        .await
        .optional()?;

    let mut builder = context.transaction_builder();

    if let Some(ValidatorScanResult { id: address, substate }) = existing_dest_account {
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
                        .fetch_substate_from_network(&SubstateId::Vault(*vault_id), None)
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
    let account_secret_key = sdk.key_manager_api().derive_account_key(account.key_index())?;

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
            .submit_dry_run_transaction(transaction)
            .await?;
        let finalize = execute_result.finalize;
        return Ok(AccountsTransferResponse {
            transaction_id,
            // TODO: technically this could cause a crash, update the api to a u64
            fee: finalize.fee_receipt.total_fees_paid,
            fee_refunded: finalize.fee_receipt.total_fee_payment - finalize.fee_receipt.total_fees_paid,
            result: finalize,
        });
    }

    // Otherwise submit and wait for a result
    let mut events = context.notifier().subscribe();
    let tx_id = context.transaction_service().submit_transaction(transaction).await?;

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
        let account = get_account_or_default(req.account.as_ref(), &sdk.accounts_api())?;

        let transfer = sdk
            .confidential_transfer_api()
            .transfer(ConfidentialTransferParams {
                from_account: *account.address(),
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
                .submit_dry_run_transaction(transfer.transaction)
                .await?;
            let finalize = exec_result.finalize;
            return Ok(ConfidentialTransferResponse {
                transaction_id,
                // TODO: technically this could cause a crash, update the api to a u64
                fee: finalize.fee_receipt.total_fees_paid,
                result: finalize,
            });
        }

        let mut events = notifier.subscribe();
        let tx_id = transaction_service.submit_transaction(transfer.transaction).await?;

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

pub async fn handle_stealth_transfer(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: StealthTransferRequest,
) -> Result<StealthTransferResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk().clone();
    let notifier = context.notifier().clone();
    let owner_account = get_account(&req.owner_account, &sdk.accounts_api())?;

    let params = StealthTransferParams {
        owner_account,
        input_selection: req.input_selection,
        destination_public_key: req.destination_public_key,
        resource_address: req.resource_address,
        max_fee: req.max_fee,
        blinded_output_amount: req.blinded_output_amount,
        revealed_output_amount: req.revealed_output_amount,
        is_dry_run: req.dry_run,
    };
    if let Err(err) = params.validate() {
        return Err(invalid_params("params", Some(err)));
    }

    let transaction_service = context.transaction_service().clone();

    // Spawn here is to prevent the async block from being aborted if the caller aborts the request early as this can
    // cause funds to remain locked indefinitely.
    task::spawn(async move {
        let transfer = sdk.stealth_transfer_api().transfer(params).await?;

        if req.dry_run {
            let transaction_id = transfer.transaction.calculate_id();
            transaction_service
                .submit_dry_run_transaction(transfer.transaction)
                .await?;
            return Ok(StealthTransferResponse { transaction_id });
        }

        let tx_id = transaction_service.submit_transaction(transfer.transaction).await?;

        notifier.notify(TransactionSubmittedEvent {
            transaction_id: tx_id,
            new_account: None,
        });

        Ok(StealthTransferResponse { transaction_id: tx_id })
    })
    .await?
}
pub async fn handle_associate_stealth_resource(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsAssociateStealthResourceRequest,
) -> Result<AccountsAssociateStealthResourceResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk().clone();
    let account = get_account(&req.account, &sdk.accounts_api())?;
    let resource = sdk.substate_api().fetch_resource(req.resource_address).await?; // validate resource exists and cache it
    if !resource.resource_type().is_stealth() {
        return Err(invalid_params(
            "resource_address",
            Some(format!(
                "Resource is not a stealth resource (type: {})",
                resource.resource_type()
            )),
        ));
    }

    sdk.accounts_api()
        .associate_stealth_resource(account.address(), req.resource_address)?;

    context.account_monitor().refresh_account(*account.address()).await?;

    Ok(AccountsAssociateStealthResourceResponse {})
}
