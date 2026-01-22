//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, iter, time::Duration};

use anyhow::{anyhow, Context};
use axum_extra::headers::authorization::Bearer;
use indexmap::{IndexMap, IndexSet};
use log::*;
use ootle_byte_type::{FromByteType, ToByteType};
use rand::rngs::OsRng;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    confidential::ClaimBurnOutputData,
    substate::SubstateId,
};
use tari_ootle_common_types::{optional::Optional, SubstateRequirement};
use tari_ootle_transaction::args;
use tari_ootle_wallet_crypto::{memo::Memo, OutputWitness, SecretStealthOutputStatement, StealthInputWitness};
use tari_ootle_wallet_sdk::{
    apis::{
        confidential_transfer::ConfidentialTransferParams,
        stealth_outputs::TransferStatementParams,
        stealth_transfer::{StealthTransferParams, TransferOutput},
        substate::ValidatorScanResult,
    },
    models::{KeyBranch, KeyId, NewAccountData, StealthUtxoSpendKeyId, TransactionSubmittedEvent},
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    models::SpendCondition,
    types::{
        constants::{STEALTH_TARI_RESOURCE_ADDRESS, XTR_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_VAULT_ADDRESS},
        Amount,
        ResourceType,
    },
};
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
        AccountsCreateStealthTransferStatementRequest,
        AccountsCreateStealthTransferStatementResponse,
        AccountsGetBalancesRequest,
        AccountsGetBalancesResponse,
        AccountsListRequest,
        AccountsListResponse,
        AccountsRenameRequest,
        AccountsRenameResponse,
        AccountsTransferRequest,
        AccountsTransferResponse,
        BalanceEntry,
        ClaimBurnProof,
        ClaimBurnRequest,
        ClaimBurnResponse,
        ConfidentialTransferRequest,
        ConfidentialTransferResponse,
        StealthTransferRequest,
        StealthTransferResponse,
    },
    ComponentAddressOrName,
};
use tokio::task;

use super::context::HandlerContext;
use crate::{
    handlers::helpers::{
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

    let owner_address = match req.key_index {
        Some(id) => sdk.key_manager_api().derive_account_address(id)?,
        None => sdk.key_manager_api().next_account_address()?,
    };

    let acc = accounts_api
        .create_account(req.account_name.as_deref(), set_as_default, owner_address)
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
        address: acc.address,
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
            .key_index
            .map(|index| get_account_by_key_index(sdk, index).optional())
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
            address: account.address,
            created: false,
        });
    }

    let set_as_default = req
        .is_default
        .map(Ok)
        .unwrap_or_else(|| accounts_api.any_accounts_exist().map(|b| !b))?;

    let wallet_keys = match req.key_index {
        Some(id) => sdk.key_manager_api().derive_account_address(id)?,
        None => sdk.key_manager_api().next_account_address()?,
    };

    let acc = accounts_api
        .create_account(req.account.as_ref().and_then(|a| a.name()), set_as_default, wallet_keys)
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
        address: acc.address,
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
    sdk.accounts_api().set_default_account(account.component_address())?;
    Ok(AccountSetDefaultResponse {})
}

pub async fn handle_rename(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsRenameRequest,
) -> Result<AccountsRenameResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let account = get_account(&req.account, &sdk.accounts_api())?;
    sdk.accounts_api()
        .rename_account(account.component_address(), &req.new_name)?;
    Ok(AccountsRenameResponse {})
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsListRequest,
) -> Result<AccountsListResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let sdk = context.wallet_sdk();
    let limit = usize::try_from(req.limit)
        .map_err(|e| invalid_params("limit", Some(&format!("limit overflowed usize: {}", e))))?;
    let offset = usize::try_from(req.offset)
        .map_err(|e| invalid_params("offset", Some(&format!("offset overflowed usize: {}", e))))?;
    let accounts_api = sdk.accounts_api();
    let accounts = accounts_api.get_many(offset, limit)?;
    let total = accounts_api.count()?;
    let accounts = accounts
        .into_iter()
        .map(|a| {
            let address = accounts_api.get_address_for_account(&a)?;
            Ok(AccountInfo {
                account: a,
                address: address.to_byte_type(),
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
    context.check_auth(token, &[JrpcPermission::AccountBalance(
        account.account.component_address.into(),
    )])?;
    if req.refresh {
        context
            .account_monitor()
            .refresh_account_with_utxos(*account.component_address())
            .await?;
    }
    let vaults = sdk.accounts_api().get_vaults_by_account(account.component_address())?;
    let stealth_outputs = sdk
        .stealth_outputs_api()
        .get_unspent_outputs_by_account(account.component_address(), false)?;

    let mut balances = Vec::with_capacity(vaults.len());
    let mut vaulted_resources = HashSet::new();
    for vault in vaults {
        let confidential_balance = if vault.resource_type.is_stealth() {
            let stealth_balance = stealth_outputs
                .iter()
                .filter(|o| o.resource_address == vault.resource_address)
                .map(|o| Amount::from(o.value))
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
                .and_modify(|v| *v += Amount::from(o.value))
                .or_insert(Amount::from(o.value));
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
        address: *account.component_address(),
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
    let account = get_account(&req.name_or_address, &sdk.accounts_api())
        .optional()?
        .ok_or_else(|| {
            not_found(format!(
                "Account with name or address '{}' not found",
                req.name_or_address
            ))
        })?;
    Ok(AccountGetResponse {
        account: account.account,
        address: account.address,
    })
}

pub async fn handle_get_by_key_index(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountGetByKeyIndexRequest,
) -> Result<AccountGetResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::AccountInfo])?;
    let sdk = context.wallet_sdk();
    let account = get_account_by_key_index(sdk, req.key_index)
        .optional()?
        .ok_or_else(|| not_found(format!("Account with key index {} not found", req.key_index)))?;
    Ok(AccountGetResponse {
        account: account.account,
        address: account.address,
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
        address: account.address,
    })
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

    let ClaimBurnProof {
        owner_nonce_key_index,
        encrypted_data: claimed_encrypted_data,
        claim_proof,
    } = claim_proof;

    let accounts_api = sdk.accounts_api();
    let account = get_account(&account, &accounts_api)?;

    let account_owner_key_id = account
        .owner_key_id()
        .ok_or_else(|| invalid_params("account", Some("cannot claim burn to an account without an owner key")))?;

    let network = sdk.config_api().get_network()?;
    // We derive secrets directly here because claim burn is a unique case, making it difficult to use the higher
    // level stealth output api that takes care of keys but assumes that this is a regular transfer.
    let claim_nonce_key = sdk
        .key_manager_api()
        .get_key(KeyId::derived(KeyBranch::Nonce, owner_nonce_key_index))?;
    let claim_public_key = claim_nonce_key.to_public_key();

    if !sdk.stealth_crypto_api().validate_burn_claim_ownership_proof(
        network,
        &claim_proof.ownership_proof,
        &claim_proof.commitment,
        claim_proof.value,
        &claim_public_key.to_byte_type(),
    ) {
        return Err(invalid_params(
            "claim_proof.ownership_proof",
            Some("ownership proof validation failed"),
        ));
    }

    info!(
        target: LOG_TARGET,
        "ℹ️ Signing claim burn with key {}. NOTE: This must be the same as the claiming key (owner_nonce_key_index) used in the burn transaction for this to succeed.",
        claim_public_key
    );

    let reciprocal_claim_public_key_expanded = claim_proof
        .burn_public_key
        .try_from_byte_type()
        .map_err(|e| invalid_params("claim_proof.reciprocal_claim_public_key", Some(e)))?;

    if reciprocal_claim_public_key_expanded != claim_public_key {
        warn!(
            target: LOG_TARGET,
            "⚠️ The provided reciprocal claim public key ({}) does not match the derived claim public key ({}). The claim will likely fail.",
            reciprocal_claim_public_key_expanded,
            claim_public_key
        );
    }

    // Get the sender_offset_public_key and use it to create a DH with the claim_nonce_key
    let sender_offset_pub_key: RistrettoPublicKey = claim_proof
        .sender_offset_public_key
        .try_from_byte_type()
        .map_err(|e| invalid_params("claim_proof.sender_offset_public_key", Some(e)))?;

    let decrypted = sdk.stealth_crypto_api().decrypt_value_and_mask(
        &claimed_encrypted_data,
        &claim_proof.commitment,
        claim_nonce_key.secret(),
        &sender_offset_pub_key,
        true,
    )?;

    let mask = sdk.key_manager_api().next_key(KeyBranch::StealthMask)?;

    let final_amount = decrypted
        .value()
        .checked_sub(max_fee)
        .ok_or_else(|| invalid_params("max_fee", Some("more fees paid than claimed amount")))?;

    if final_amount == 0 {
        return Err(invalid_params("max_fee", Some("fee equals or exceeds claimed amount")));
    }

    let (nonce, output_public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
    let account_owner = sdk.key_manager_api().get_key(account_owner_key_id)?;
    let account_owner_public_key = account_owner.to_public_key();
    let view_only = sdk.key_manager_api().get_key(account.view_only_key_id())?;
    let view_only_public_key = view_only.to_public_key();
    let memo = Memo::new_message("Claimed burned XTR from L1").expect("valid memo");
    // NOTE: the confidential encryption format and the bullet proofs currently do not support amounts larger than
    // u64::MAX. Apart from it being insane/basically impossible to have that much XTR in a single UTXO, the L1 emission
    // will reach this much in many thousands of years.
    let encrypted_data = sdk.stealth_crypto_api().encrypt_value_and_mask(
        final_amount,
        &mask.key,
        &view_only_public_key,
        &nonce,
        Some(&memo),
    )?;

    let tag = sdk.stealth_crypto_api().derive_stealth_output_tag(
        network,
        &nonce,
        &view_only_public_key,
        &STEALTH_TARI_RESOURCE_ADDRESS,
    );

    // Create stealth address - used during spend time
    let stealth_output_owner_public_key =
        sdk.stealth_crypto_api()
            .derive_stealth_owner_public_key(network, &account_owner_public_key, &nonce);

    let output_statement = SecretStealthOutputStatement {
        witness: OutputWitness {
            amount: final_amount,
            mask: mask.key,
            sender_public_nonce: output_public_nonce.clone(),
            minimum_value_promise: 0,
            encrypted_data,
            resource_view_key: None,
        },
        spend_condition: SpendCondition::Signed(stealth_output_owner_public_key.to_byte_type()),
        tag,
    };

    // Package the secrets required to spend the claimed output
    let input = StealthInputWitness {
        mask_and_value: decrypted.into_mask_and_value(),
        public_nonce: reciprocal_claim_public_key_expanded,
    };

    let pay_fee_and_mint_output = sdk.stealth_crypto_api().generate_transfer_statement(
        iter::once(input),
        0,
        iter::once(&output_statement),
        max_fee,
        // public_signer_key.public_key.to_byte_type(),
    )?;
    // We'll create an output with the same encrypted data that was used on L1 burn. Note that this is not strictly
    // necessary. The engine will create the output with whatever you give it, so we could reencrypt.
    let output_data = ClaimBurnOutputData {
        encrypted_data: claimed_encrypted_data,
    };

    let transaction = context
        .transaction_builder()
        .with_fee_instructions_builder(|fee_builder| {
            fee_builder
                .claim_burn(claim_proof, output_data)
                .pay_fee_stealth(pay_fee_and_mint_output)
        })
        .finish();

    // Add the required spend signature to the transaction
    let transaction = sdk.signer_api().sign(*claim_nonce_key.key_id(), transaction)?;

    let tx_id = context.transaction_service().submit_transaction(transaction).await?;

    Ok(ClaimBurnResponse { transaction_id: tx_id })
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

    let account_owner_key_id = account.owner_key_id().ok_or_else(|| {
        invalid_params(
            "account",
            Some("cannot create free test coins for an account without an owner key"),
        )
    })?;

    info!(
        target: LOG_TARGET,
        "💰️ Creating free test coins for account: {} with amount: {} and max fee: {}",
        account.account.component_address,
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
            account.account.component_address
        );
        // Add account inputs
        let account_substate = sdk
            .substate_api()
            .get_substate(&account.account.component_address.into())?;
        inputs.push(account_substate.substate_id.into());

        // Add all versioned account child addresses as inputs
        let child_addresses = sdk
            .substate_api()
            .load_dependent_substates(&[&account.account.component_address.into()])?;
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
            account.account.component_address
        );
    }

    let transaction = context
        .transaction_builder()
        .with_fee_instructions_builder(|fee_builder| {
            fee_builder
                .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![amount])
                .put_last_instruction_output_on_workspace("faucet_funds")
                .create_account_with_bucket(*account.address.account_public_key(), "faucet_funds")
                .put_last_instruction_output_on_workspace("new_account")
                .call_method("new_account", "pay_fee", args![max_fee])
        })
        .with_inputs(inputs.into_iter().map(|input| input.into_unversioned()))
        .finish();

    let transaction = sdk.signer_api().sign(account_owner_key_id, transaction)?;

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
                address: *account.component_address(),
            }),
            None,
        )
        .await?;

    // Wait for the monitor to pick up the new or updated account
    let (finalized, _) = wait_for_result_and_account(&mut events, &tx_id, account.component_address()).await?;
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
        .get_account_by_address(account.component_address())
        .optional()?
        .ok_or_else(|| {
            not_found(format!(
                "Account with address '{}' not found",
                account.component_address()
            ))
        })?;

    Ok(AccountsCreateFreeTestCoinsResponse {
        account: account.account,
        transaction_id: tx_id,
        amount,
        fee: max_fee,
        result: finalized.finalize,
        address: account.address,
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

    let account_owner_key_id = account
        .owner_key_id()
        .ok_or_else(|| invalid_params("account", Some("cannot transfer from an account without an owner key")))?;

    // get the source account component address
    let source_account_address = *account.component_address();

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

    let builder = context.transaction_builder().create_account(req.destination_public_key);

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
    }

    // build the transaction
    let max_fee = req.max_fee.unwrap_or(DEFAULT_FEE);

    let transaction = builder
        .with_dry_run(req.dry_run)
        .pay_fee_from_component(source_account_address, max_fee)
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
        .finish();

    let transaction = sdk.signer_api().sign(account_owner_key_id, transaction)?;

    // If dry run we can return the result immediately
    if req.dry_run {
        let transaction_id = transaction.calculate_id();
        let _execute_result = context
            .transaction_service()
            .submit_dry_run_transaction(transaction)
            .await?;
        return Ok(AccountsTransferResponse { transaction_id });
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

    Ok(AccountsTransferResponse { transaction_id: tx_id })
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
                from_account: *account.component_address(),
                input_selection: req.input_selection,
                amount: req.amount,
                destination_address: req.destination_address,
                resource_address: req.resource_address,
                max_fee: req.max_fee.unwrap_or(DEFAULT_FEE),
                output_to_revealed: req.output_to_revealed,
                proof_from_resource: req.proof_from_badge_resource,
                memo: req.memo,
                is_dry_run: req.dry_run,
            })
            .await?;

        if req.dry_run {
            let transaction_id = transfer.transaction.calculate_id();
            let _exec_result = transaction_service
                .submit_dry_run_transaction(transfer.transaction)
                .await?;
            return Ok(ConfidentialTransferResponse { transaction_id });
        }

        let tx_id = transaction_service.submit_transaction(transfer.transaction).await?;

        notifier.notify(TransactionSubmittedEvent {
            transaction_id: tx_id,
            new_account: None,
        });

        Ok(ConfidentialTransferResponse { transaction_id: tx_id })
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
    let network = sdk.sdk_config().network;
    let notifier = context.notifier().clone();
    let owner_account = get_account(&req.owner_account, &sdk.accounts_api())?;
    if owner_account.owner_key_id().is_none() {
        return Err(invalid_params(
            "owner_account",
            Some("cannot transfer from an account without an owner key"),
        ));
    };

    let outputs = req
        .transfers
        .into_iter()
        .map(|transfer| match transfer.destination_address.pay_ref() {
            Some(pay_ref) => {
                let memo = transfer.output_memo.unwrap_or_else(|| Memo::new_message("").unwrap());
                if memo.as_pay_ref().is_some() {
                    warn!(
                        target: LOG_TARGET,
                        "❗️ Overwriting existing pay ref in memo for transfer to address {}",
                        transfer.destination_address
                    );
                }

                // Try to add the pay ref to the memo
                let memo_bytes = memo
                    .as_memo_message()
                    .map(|s| s.as_bytes())
                    .or_else(|| memo.as_memo_bytes())
                    .ok_or_else(|| invalid_params("pay ref", Some("can only include pay ref in message memo")))?;
                let memo = Memo::new_pay_ref_and_bytes_truncate(pay_ref, memo_bytes)
                    .expect("payref + truncated message fits in memo");

                Ok(TransferOutput {
                    address: transfer.destination_address,
                    blinded_amount: transfer.blinded_output_amount,
                    revealed_amount: transfer.revealed_output_amount,
                    memo: Some(memo),
                    pay_to: transfer.pay_to,
                })
            },
            None => Ok(TransferOutput {
                address: transfer.destination_address,
                blinded_amount: transfer.blinded_output_amount,
                revealed_amount: transfer.revealed_output_amount,
                memo: transfer.output_memo,
                pay_to: transfer.pay_to,
            }),
        })
        .collect::<anyhow::Result<_>>()?;

    let params = StealthTransferParams {
        fee_input_selection: req.fee_input_selection,
        input_selection: req.input_selection,
        resource_address: req.resource_address,
        max_fee: req.max_fee,
        badge_usage: req.badge_usage,
        outputs,
        is_dry_run: req.dry_run,
    };
    if let Err(err) = params.validate(network) {
        return Err(invalid_params("params", Some(err)));
    }

    let transaction_service = context.transaction_service().clone();

    // Spawn here is to prevent the async block from being aborted if the caller aborts the request early as this can
    // cause funds to remain locked indefinitely.
    task::spawn(async move {
        let (lock, transfer) = sdk.stealth_transfer_api().transfer(owner_account, params).await?;

        let transaction = transfer.transaction;
        let main_pk = transfer.main_signer.public_key().to_byte_type();

        // Signer api which sign transaction types that require the seal signer public key
        let main_signer = sdk.signer_api().with_context(&main_pk);
        // Add additional signature if needed
        let transaction = match transfer.additional_signer.as_ref() {
            Some(s) => main_signer.sign(s.key_id, transaction)?,
            None => transaction.finish(),
        };

        // Add required UTXO spend key signatures
        let transaction = transfer
            .utxo_spend_keys
            .iter()
            .try_fold(transaction, |tx, key| main_signer.sign_with_stealth_key(key, tx))?;

        // Sign and seal the final transaction
        let transaction = sdk.signer_api().sign(transfer.main_signer.key_id, transaction)?;

        if req.dry_run {
            // Release the lock immediately as dry run does not submit the transaction
            // TODO: maybe transfer() should not lock the outputs if it's a dry run
            lock.release();
            let result = transaction_service.submit_dry_run_transaction(transaction).await;
            return match result {
                Ok(res) => Ok(StealthTransferResponse {
                    transaction_id: res.finalize.transaction_hash.into(),
                }),
                Err(e) => Err(anyhow::anyhow!("Dry run transaction failed: {}", e)),
            };
        }

        let tx_id = transaction_service
            .submit_transaction_with_opts(transaction, None, Some(lock.id()))
            .await
            .context("Transaction failed to submit")?;

        // Transaction submitted, we're home free, make sure to allow the lock to persist past this call.
        // The wallet will monitor the transaction and release the lock when it's finalized.
        lock.keep_locked();

        notifier.notify(TransactionSubmittedEvent {
            transaction_id: tx_id,
            new_account: None,
        });

        Ok(StealthTransferResponse { transaction_id: tx_id })
    })
    .await?
}

#[allow(clippy::too_many_lines)]
pub async fn handle_create_stealth_transfer_statement(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: AccountsCreateStealthTransferStatementRequest,
) -> Result<AccountsCreateStealthTransferStatementResponse, anyhow::Error> {
    context.check_auth(token, &[JrpcPermission::TransactionSend(None)])?;
    let sdk = context.wallet_sdk().clone();
    if req.requests.is_empty() {
        return Err(invalid_params(
            "requests",
            Some("at least one transfer request must be provided"),
        ));
    }

    if req.requests.len() > 16 {
        return Err(invalid_params(
            "requests",
            Some("a maximum of 16 transfer requests can be processed at once"),
        ));
    }

    let mut required_signers = HashSet::new();
    let mut utxo_signers = IndexSet::new();
    let lock = sdk.locks_api().create_lock_with_timeout(Duration::from_secs(5 * 60))?;
    let mut statements = Vec::with_capacity(req.requests.len());
    for req in req.requests {
        let sender_account = get_account(&req.sender_account, &sdk.accounts_api())?;
        let Some(sender_key_id) = sender_account.owner_key_id() else {
            return Err(invalid_params(
                "owner_account",
                Some("cannot transfer from an account without an owner key"),
            ));
        };

        let resource = sdk.substate_api().fetch_resource(req.resource_address).await?;

        if !resource.resource_type().is_stealth() {
            return Err(invalid_params(
                "resource_address",
                Some(format!(
                    "Resource is not a stealth resource (type: {})",
                    resource.resource_type()
                )),
            ));
        }

        let amount_to_spend = req.total_output_amount();

        let inputs = req
            .input_selection
            .as_selection()
            .map(|sel| {
                sdk.stealth_transfer_api().lock_inputs_for_transfer(
                    lock.id(),
                    sender_account.component_address(),
                    req.resource_address,
                    amount_to_spend,
                    sel,
                )
            })
            .transpose()?;

        let must_sign_with_account_key = inputs.as_ref().is_some_and(|i| i.revealed.is_positive());
        let signing_key_id = if must_sign_with_account_key {
            sender_key_id
        } else {
            sdk.key_manager_api().next_derived_key_id(KeyBranch::Nonce)?.into()
        };

        let output_revealed_amount = req.outputs.iter().map(|o| o.revealed_amount).sum();
        let outputs = req
            .outputs
            .iter()
            .filter(|o| o.blinded_amount > 0)
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        let statement = sdk
            .stealth_outputs_api()
            .generate_transfer_statement(TransferStatementParams {
                view_only_key_id: sender_account.view_only_key_id(),
                resource_address: &req.resource_address,
                resource_view_key: resource
                    .to_view_key_public_key()
                    .map_err(|e| anyhow!("Failed to decode resource public view key: {e}"))?,
                inputs: inputs.as_ref().map(|i| i.inputs.as_slice()).unwrap_or(&[]),
                input_revealed_amount: req
                    .input_selection
                    .as_from_bucket()
                    .unwrap_or(Amount::zero())
                    .checked_add_positive(inputs.as_ref().map(|i| i.revealed).unwrap_or(Amount::zero()))
                    .ok_or_else(|| {
                        invalid_params(
                            "input_revealed_amount",
                            Some("input revealed amount overflowed or was negative"),
                        )
                    })?,
                outputs,
                output_revealed_amount,
            })?;

        utxo_signers.extend(inputs.iter().flat_map(|i| &i.inputs).map(|i| StealthUtxoSpendKeyId {
            account_key_id: sender_key_id,
            public_nonce: i.public_nonce,
        }));

        required_signers.insert(signing_key_id);
        statements.push(statement);
    }

    // Return without unlocking the outputs
    let lock_id = lock.keep_locked();

    Ok(AccountsCreateStealthTransferStatementResponse {
        statements,
        lock_id,
        signing_keys: required_signers.into_iter().collect(),
        utxo_signers: utxo_signers.into_iter().collect(),
    })
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

    context
        .account_monitor()
        .associate_resource(*account.component_address(), req.resource_address)
        .await?;

    context
        .account_monitor()
        .refresh_account_with_utxos(*account.component_address())
        .await?;

    Ok(AccountsAssociateStealthResourceResponse {})
}
