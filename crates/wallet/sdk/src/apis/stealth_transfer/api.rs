//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{cmp, collections::HashSet, time::Duration};

use log::*;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::{substate::SubstateId, ConvertFromByteType, FromByteType};
use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_common_types::{displayable::Displayable, optional::Optional, Network, SubstateRequirement};
use tari_ootle_wallet_crypto::memo::Memo;
use tari_template_lib::{
    constants::XTR,
    models::{
        Account as BuiltinAccount,
        ComponentAddress,
        ResourceAddress,
        StealthTransferStatement,
        StealthUnspentOutput,
        UtxoAddress,
    },
    types::Amount,
};
use tari_transaction::{args, Transaction, UnsignedTransaction};
use tokio::{sync::Semaphore, task::block_in_place};

use super::{
    error::StealthTransferApiError,
    params::StealthTransferParams,
    types::{InputsToSpend, StealthOutputToCreate, StealthTransferOutput},
    BadgeUsage,
    PayTo,
    TransferOutput,
};
use crate::{
    apis::{
        accounts::{derive_account_address_from_public_key, AccountsApi},
        confidential_transfer::UtxoInputSelection,
        config::ConfigApi,
        key_manager::KeyManagerApi,
        locks::LocksApi,
        stealth_outputs::{StealthOutputsApi, StealthOutputsApiError, TransferStatementParams},
        substate::{SubstatesApi, ValidatorScanResult},
    },
    models::{
        AccountWithAddress,
        KeyBranch,
        OutputStatus,
        StealthOutputModel,
        StealthUtxoSpendKeyId,
        WalletLockDropGuard,
        WalletLockId,
    },
    WalletSdkSpec,
};

const LOG_TARGET: &str = "tari::ootle::wallet_sdk::apis::stealth_transfers";

pub struct StealthTransferApi<'a, TSpec: WalletSdkSpec> {
    accounts_api: AccountsApi<'a, TSpec>,
    outputs_api: StealthOutputsApi<'a, TSpec>,
    locks_api: LocksApi<'a, TSpec::Store>,
    substate_api: SubstatesApi<'a, TSpec::Store, TSpec::NetworkInterface>,
    key_manager_api: KeyManagerApi<'a, TSpec>,
    config_api: ConfigApi<'a, TSpec::Store>,
    semaphore: Semaphore,
}

impl<'a, TSpec: WalletSdkSpec> StealthTransferApi<'a, TSpec> {
    pub fn new(
        accounts_api: AccountsApi<'a, TSpec>,
        outputs_api: StealthOutputsApi<'a, TSpec>,
        locks_api: LocksApi<'a, TSpec::Store>,
        substate_api: SubstatesApi<'a, TSpec::Store, TSpec::NetworkInterface>,
        key_manager_api: KeyManagerApi<'a, TSpec>,
        config_api: ConfigApi<'a, TSpec::Store>,
    ) -> Self {
        Self {
            accounts_api,
            outputs_api,
            locks_api,
            substate_api,
            key_manager_api,
            config_api,
            semaphore: Semaphore::new(1),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn lock_inputs_for_transfer(
        &self,
        lock_id: WalletLockId,
        owner_account_component_address: &ComponentAddress,
        resource_address: ResourceAddress,
        spend_amount: Amount,
        input_selection: UtxoInputSelection,
    ) -> Result<InputsToSpend, StealthTransferApiError> {
        if !spend_amount.is_positive() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "spend_amount",
                reason: "Spend amount must be positive".to_string(),
            });
        }

        let maybe_src_vault = self
            .accounts_api
            .get_vault_by_resource(owner_account_component_address, &resource_address)
            .optional()?;

        let available_revealed_funds = maybe_src_vault
            .as_ref()
            .map(|v| v.available_revealed_balance())
            .unwrap_or_else(Amount::zero);

        match input_selection {
            UtxoInputSelection::ConfidentialOnly => {
                let (inputs, total_locked) = self.outputs_api.lock_outputs_for_at_least_amount(
                    owner_account_component_address,
                    &resource_address,
                    lock_id,
                    spend_amount,
                )?;

                info!(
                    target: LOG_TARGET,
                    "ConfidentialOnly: Locked {} confidential inputs for transfer worth {}",
                    inputs.len(),
                    total_locked
                );

                Ok(InputsToSpend {
                    inputs,
                    revealed: Amount::zero(),
                })
            },
            UtxoInputSelection::RevealedOnly => {
                if available_revealed_funds < spend_amount {
                    return Err(StealthTransferApiError::InsufficientFunds {
                        details: "RevealedOnly: Not enough revealed funds to spend.".to_string(),
                        available: available_revealed_funds,
                        required: spend_amount,
                    });
                }

                let src_vault =
                    maybe_src_vault
                        .as_ref()
                        .ok_or_else(|| StealthTransferApiError::InsufficientRevealedFunds {
                            details: format!(
                                "No vault found for resource {} in account {}",
                                resource_address, owner_account_component_address
                            ),
                        })?;

                self.locks_api
                    .lock_funds_in_vault(lock_id, &src_vault.id, spend_amount)?;

                info!(
                    target: LOG_TARGET,
                    "RevealedOnly: Spending {} revealed balance for transfer from {}",
                    spend_amount,
                    maybe_src_vault.as_ref().map(|v| v.id).display()
                );

                Ok(InputsToSpend {
                    inputs: vec![],
                    revealed: spend_amount,
                })
            },
            UtxoInputSelection::PreferRevealed => {
                let revealed_to_spend = cmp::min(available_revealed_funds, spend_amount);
                let utxo_amount_to_spend = spend_amount - revealed_to_spend;
                if let Some(ref src_vault) = maybe_src_vault {
                    if utxo_amount_to_spend.is_zero() {
                        info!(
                            target: LOG_TARGET,
                            "PreferRevealed: Spending {} revealed balance (available: {}) for transfer from {}",
                            revealed_to_spend,
                            available_revealed_funds,
                            src_vault.id
                        );

                        self.locks_api
                            .lock_funds_in_vault(lock_id, &src_vault.id, revealed_to_spend)?;

                        return Ok(InputsToSpend {
                            inputs: vec![],
                            revealed: revealed_to_spend,
                        });
                    }
                }

                if maybe_src_vault.is_none() && revealed_to_spend.is_positive() {
                    // No vault containing revealed funds was found
                    return Err(StealthTransferApiError::InsufficientRevealedFunds {
                        details: format!(
                            "PreferRevealed: No vault found for resource {} in account {}. Need to spend {} revealed \
                             funds",
                            resource_address, owner_account_component_address, revealed_to_spend
                        ),
                    });
                }

                let (inputs, _) = self.outputs_api.lock_outputs_for_at_least_amount(
                    owner_account_component_address,
                    &resource_address,
                    lock_id,
                    utxo_amount_to_spend,
                )?;

                let total_confidential_spent = inputs.iter().map(|i| Amount::from(i.value)).sum::<Amount>();

                if let Some(ref src_vault) = maybe_src_vault {
                    self.locks_api
                        .lock_funds_in_vault(lock_id, &src_vault.id, revealed_to_spend)?;
                }

                info!(
                    target: LOG_TARGET,
                    "PreferRevealed: Locked {} confidential inputs (target: {}, spent: {}) and {} revealed for amount {} from {}",
                    inputs.len(),
                    utxo_amount_to_spend,
                    total_confidential_spent,
                    revealed_to_spend,
                    spend_amount,
                    maybe_src_vault.as_ref().map(|v| v.id).display()
                );

                Ok(InputsToSpend {
                    inputs,
                    revealed: revealed_to_spend,
                })
            },
            UtxoInputSelection::PreferConfidential => {
                let (inputs, blinded_amount_locked) = self.outputs_api.lock_outputs_until_partial_amount(
                    owner_account_component_address,
                    &resource_address,
                    spend_amount,
                    lock_id,
                )?;

                let revealed_to_spend = spend_amount.saturating_sub_positive(blinded_amount_locked);

                if available_revealed_funds < revealed_to_spend {
                    return Err(StealthTransferApiError::InsufficientFunds {
                        details: "PreferConfidential: Not enough revealed funds to spend.".to_string(),
                        available: available_revealed_funds,
                        required: revealed_to_spend,
                    });
                }

                if revealed_to_spend.is_positive() {
                    let vault =
                        maybe_src_vault
                            .as_ref()
                            .ok_or_else(|| StealthTransferApiError::InsufficientRevealedFunds {
                                details: format!(
                                    "PreferConfidential: No vault found for resource {} in account {}. Need to spend \
                                     {} revealed funds",
                                    resource_address, owner_account_component_address, revealed_to_spend
                                ),
                            })?;

                    self.locks_api
                        .lock_funds_in_vault(lock_id, &vault.id, revealed_to_spend)?;
                }

                Ok(InputsToSpend {
                    inputs,
                    revealed: revealed_to_spend,
                })
            },
        }
    }

    fn lock_fee_inputs<A: Into<Amount>>(
        &self,
        lock_id: WalletLockId,
        owner_account: &AccountWithAddress,
        max_fee: A,
        input_selection: UtxoInputSelection,
    ) -> Result<InputsToSpend, StealthTransferApiError> {
        self.lock_inputs_for_transfer(
            lock_id,
            owner_account.account().component_address(),
            XTR,
            max_fee.into(),
            input_selection,
        )
    }

    #[allow(clippy::too_many_lines)]
    pub async fn transfer(
        &self,
        owner_account: AccountWithAddress,
        params: StealthTransferParams,
    ) -> Result<(WalletLockDropGuard<'a, TSpec::Store>, StealthTransferOutput), StealthTransferApiError> {
        let network = self.config_api.get_network()?;
        params.validate(network)?;

        let Some(account_key_id) = owner_account.owner_key_id() else {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "owner_account",
                reason: format!(
                    "Account {} is view only and does not have the required secrets for transfers",
                    owner_account.component_address()
                ),
            });
        };

        // Determine Transaction Inputs
        let mut substate_inputs = Vec::new();
        let owner_address: RistrettoOotleAddress =
            owner_account
                .address()
                .try_from_byte_type()
                .map_err(|e| StealthTransferApiError::InvalidParameter {
                    param: "owner_account",
                    reason: format!("Non-canonical owner account address: {e}"),
                })?;

        // add the input for the resource address to be transferred
        substate_inputs.push(SubstateRequirement::unversioned(params.resource_address));

        let mut accounts_to_create = HashSet::new();
        for output in &params.outputs {
            let need_to_create_dest_account = self
                .determine_destination_account_inputs(output, &params.resource_address, &mut substate_inputs)
                .await?;
            if need_to_create_dest_account {
                let dest_account = derive_account_address_from_public_key(output.address.account_public_key());
                accounts_to_create.insert(dest_account);
            }
        }

        // We need to fetch the resource substate to check if there is a view key present.
        let resource = self.substate_api.fetch_resource(params.resource_address).await?;

        // Generate outputs
        let resource_view_key = resource
            .view_key()
            .map(RistrettoPublicKey::convert_from_byte_type)
            .transpose()
            .map_err(|e| StealthTransferApiError::InvalidParameter {
                param: "resource_view_key",
                reason: format!("Invalid resource view key: {e}"),
            })?;

        // Critical section - TODO: use a DB transaction
        let _permit = self.semaphore.acquire().await.expect("semaphore is never closed");

        block_in_place(|| {
            // Create a lock with a timeout, the lock timeout will be removed if the lock is assigned a transaction
            let lock = self.locks_api.create_lock_with_timeout(Duration::from_secs(5 * 60))?;

            // Lock up funds for fees and transfer
            let fee_inputs_to_spend =
                self.lock_fee_inputs(lock.id(), &owner_account, params.max_fee, params.fee_input_selection)?;

            debug!(
                target: LOG_TARGET,
                "🔒️ Locked {} fee inputs for fee spending worth {} (max fee {})",
                fee_inputs_to_spend.inputs.len(),
                fee_inputs_to_spend.total_stealth_input_amount(),
                params.max_fee,
            );

            let fee_stealth_change_amt = fee_inputs_to_spend
                .total_stealth_input_amount()
                .saturating_sub_positive(params.max_fee.into())
                .to_u64_checked()
                .ok_or_else(|| {
                    StealthTransferApiError::InvariantViolation {
                        // Technically, you could create multiple outputs, but for simplicity and because this is
                        // extremely unlikely to be needed, we only create one here
                        details: "Fee change amount exceeds u64".to_string(),
                    }
                })?;

            // Generate fee change outputs if required
            let fee_change_output = Some(StealthOutputToCreate {
                owner_address: owner_address.clone(),
                amount: fee_stealth_change_amt,
                memo: None,
                pay_to: PayTo::StealthPublicKey,
            })
            .filter(|o| o.amount > 0);

            // Figure out which signing key to use - if there are no revealed funds, which necessitate using an account
            // withdraw auth signature, then we can use a nonce key.
            let must_sign_with_account_key = fee_inputs_to_spend.revealed.is_positive();
            let signing_key_id = if must_sign_with_account_key {
                account_key_id
            } else {
                self.key_manager_api.next_derived_key_id(KeyBranch::Nonce)?.into()
            };
            let fee_signer = self.key_manager_api.get_public_key(signing_key_id)?;

            // Generate fee transfer statement
            let fee_transfer_statement = self.outputs_api.generate_transfer_statement(TransferStatementParams {
                view_only_key_id: owner_account.view_only_key_id(),
                resource_address: &params.resource_address,
                resource_view_key: None,
                inputs: &fee_inputs_to_spend.inputs,
                input_revealed_amount: fee_inputs_to_spend.revealed,
                outputs: fee_change_output,
                output_revealed_amount: Amount::from(params.max_fee),
            })?;

            // Add the unconfirmed fee change output to the wallet store
            if let Some(output) = fee_transfer_statement.outputs_statement.outputs.first() {
                debug!(
                    target: LOG_TARGET,
                    "Adding FEE unconfirmed output with commitment {} for amount {} to account {}",
                    output.output.commitment,
                    fee_stealth_change_amt,
                    owner_account.component_address()
                );
                self.add_unconfirmed_output_from_statement(
                    lock.id(),
                    &owner_account,
                    XTR,
                    output,
                    fee_stealth_change_amt,
                    None,
                )?;
            }

            // NOTE: important to add this after we add the fee change, because this allows us to spend the fee change
            // UTXO (XTR case)
            let inputs_to_spend = self.lock_inputs_for_transfer(
                lock.id(),
                owner_account.account().component_address(),
                params.resource_address,
                params.total_output_amount(),
                params.input_selection,
            )?;

            // Signing key for main transfer intent
            let must_sign_with_account_key = !params.badge_usage.is_none() || inputs_to_spend.revealed.is_positive();
            let signing_key_id = if must_sign_with_account_key {
                account_key_id
            } else {
                self.key_manager_api.next_derived_key_id(KeyBranch::Nonce)?.into()
            };

            // No need to add another signature if the fee signer is the same as the main signer
            let main_intent_signer = if fee_signer.key_id() == signing_key_id {
                None
            } else {
                let required_signer = self.key_manager_api.get_public_key(signing_key_id)?;
                Some(required_signer)
            };

            // If we're spending from the owner account, add the inputs
            if inputs_to_spend.revealed.is_positive() || fee_inputs_to_spend.revealed.is_positive() {
                substate_inputs.push(SubstateRequirement::unversioned(*owner_account.component_address()));

                // Add the vaults for XTR (fees) and the spending resource if different
                if let Some(vault) = self
                    .accounts_api
                    .get_vault_by_resource(owner_account.component_address(), &XTR)
                    .optional()?
                {
                    substate_inputs.push(SubstateRequirement::unversioned(vault.id));
                    substate_inputs.push(SubstateRequirement::unversioned(vault.resource_address));
                }
                if params.resource_address != XTR {
                    if let Some(vault) = self
                        .accounts_api
                        .get_vault_by_resource(owner_account.component_address(), &params.resource_address)
                        .optional()?
                    {
                        substate_inputs.push(SubstateRequirement::unversioned(vault.id));
                        substate_inputs.push(SubstateRequirement::unversioned(vault.resource_address));
                    }
                }
            }

            // Any change outputs?
            let change_amount = inputs_to_spend
                .total_amount()
                .checked_sub_positive(params.total_output_amount())
                .ok_or_else(|| StealthTransferApiError::InvariantViolation {
                    details: format!(
                        "Total input amount {} is less than total output amount {}",
                        inputs_to_spend.total_amount(),
                        params.total_output_amount()
                    ),
                })?;

            let change_output = Some(StealthOutputToCreate {
                owner_address,
                amount: change_amount
                    .to_u64_checked()
                    .ok_or_else(|| StealthTransferApiError::InvariantViolation {
                        // Technically, you could create multiple outputs, but for simplicity and because this is
                        // extremely unlikely to be needed, we only create one here
                        details: "Change amount exceeds u64".to_string(),
                    })?,
                memo: None,
                pay_to: PayTo::StealthPublicKey,
            });

            let output_revealed_amount = params.total_revealed_output_amount();
            let outputs_to_create = params
                .outputs
                .iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, StealthOutputsApiError>>()?;

            let transfer_statement = self.outputs_api.generate_transfer_statement(TransferStatementParams {
                view_only_key_id: owner_account.view_only_key_id(),
                resource_address: &params.resource_address,
                resource_view_key,
                inputs: &inputs_to_spend.inputs,
                input_revealed_amount: inputs_to_spend.revealed,
                outputs: outputs_to_create
                    .into_iter()
                    .chain(change_output)
                    .filter(|o| o.amount > 0),
                output_revealed_amount,
            })?;

            // Add the unconfirmed change output to the wallet store
            // NOTE: we can get the nth element because outputs are guaranteed to be in the order we pass them to
            // generate_transfer_statement
            if change_amount.is_positive() {
                if let Some(output) = transfer_statement.outputs_statement.outputs.last() {
                    debug!(
                    target: LOG_TARGET,
                    "Adding TRANSFER unconfirmed output with commitment {} for amount {} to account {}",
                    output.output.commitment,
                    change_amount,
                    owner_account.component_address()
                    );
                    self.add_unconfirmed_output_from_statement(
                        lock.id(),
                        &owner_account,
                        params.resource_address,
                        output,
                        change_amount
                            .to_u64_checked()
                            .ok_or_else(|| StealthTransferApiError::InvariantViolation {
                                details: "Change amount exceeds u64".to_string(),
                            })?,
                        None,
                    )?;
                }
            }

            // Add all input UTXO substates to transaction inputs
            substate_inputs.extend(
                fee_inputs_to_spend
                    .inputs
                    .iter()
                    // If spending XTR, we may lock the fee change UTXO for spending, however since this does not exist yet, we do not include it as a tx input
                    .filter(|i| i.is_on_chain)
                    .map(|i| &i.commitment)
                    .map(|commitment| UtxoAddress::new(XTR, (*commitment).into()))
                    .map(SubstateRequirement::unversioned),
            );

            substate_inputs.extend(
                inputs_to_spend
                    .inputs
                    .iter()
                    .filter(|i| i.is_on_chain)
                    .map(|i| &i.commitment)
                    .map(|commitment| UtxoAddress::new(params.resource_address, (*commitment).into()))
                    .map(SubstateRequirement::unversioned),
            );

            // Add badge vault if needed
            if let Some(badge_resource_address) = params.badge_usage.resource_address() {
                let badge_vault = self
                    .accounts_api
                    .get_vault_by_resource(owner_account.component_address(), badge_resource_address)
                    .optional()?
                    .ok_or_else(|| StealthTransferApiError::BadgeVaultNotFound {
                        resource_address: *badge_resource_address,
                    })?;
                substate_inputs.push(SubstateRequirement::unversioned(badge_vault.id));
            }

            // We assume that all inputs being spent require a signature. This is fine because we currently filter out
            // inputs that have complex access rules from input selection.
            let utxo_spend_keys = inputs_to_spend
                .inputs
                .iter()
                .chain(&fee_inputs_to_spend.inputs)
                .map(|i| StealthUtxoSpendKeyId {
                    account_key_id,
                    public_nonce: i.public_nonce,
                })
                .collect();

            let transaction = self.generate_transfer_transaction(
                network,
                &owner_account,
                params,
                substate_inputs,
                fee_transfer_statement,
                transfer_statement,
                &accounts_to_create,
            )?;

            Ok((lock, StealthTransferOutput {
                transaction,
                fee_inputs: fee_inputs_to_spend,
                transfer_inputs: inputs_to_spend,
                utxo_spend_keys,
                additional_signer: main_intent_signer,
                main_signer: fee_signer,
            }))
        })
    }

    async fn determine_destination_account_inputs(
        &self,
        output: &TransferOutput,
        resource_address: &ResourceAddress,
        substate_inputs: &mut Vec<SubstateRequirement>,
    ) -> Result<bool, StealthTransferApiError> {
        // No revealed outputs, no need to use the account
        if !output.revealed_amount.is_positive() {
            return Ok(false);
        }

        let destination_account = derive_account_address_from_public_key(output.address.account_public_key());

        // Local account? (Saves a call to the indexer)
        match self
            .accounts_api
            .get_account_by_address(&destination_account)
            .optional()?
        {
            Some(local_account) => {
                if local_account.is_confirmed_on_chain() {
                    substate_inputs.push(SubstateRequirement::unversioned(destination_account));
                    if let Some(vault) = self
                        .accounts_api
                        .get_vault_by_resource(local_account.component_address(), resource_address)
                        .optional()?
                    {
                        substate_inputs.push(SubstateRequirement::unversioned(vault.id));
                    }

                    Ok(false)
                } else {
                    Ok(true)
                }
            },
            None => {
                // TODO: we're just determining if the account exists - symptom of a larger problem/missing
                // feature: the account should be created as needed by the execution layer, instead of having to be
                // determined by the client side
                let to_account_substate = self
                    .substate_api
                    .fetch_substate_from_network(&SubstateId::Component(destination_account), None)
                    .await
                    .optional()?;

                if let Some(ValidatorScanResult { id: address, substate }) = to_account_substate {
                    substate_inputs.push(SubstateRequirement::unversioned(destination_account));

                    let account =
                        substate
                            .component()
                            .ok_or_else(|| StealthTransferApiError::UnexpectedIndexerResponse {
                                details: format!(
                                    "Expected indexer to return component for address {}. It returned {}",
                                    destination_account, address
                                ),
                            })?;
                    let dest_account = BuiltinAccount::from_value(account.state()).map_err(|e| {
                        StealthTransferApiError::UnexpectedIndexerResponse {
                            details: format!("Failed to convert component substate to account: {e}"),
                        }
                    })?;
                    // If they have an existing vault, we need to add it as an input
                    if let Some(vault) = dest_account.get_vault_by_resource(resource_address) {
                        debug!(
                            target: LOG_TARGET,
                            "Found existing vault {} for resource {} in destination account {}",
                            vault.vault_id(),
                            resource_address,
                            destination_account
                        );
                        substate_inputs.push(SubstateRequirement::unversioned(vault.vault_id()));
                    } else {
                        debug!(
                            target: LOG_TARGET,
                            "No existing vault found for resource {} in destination account {}. It will be created.",
                            resource_address,
                            destination_account
                        );
                    }
                    Ok(false)
                } else {
                    // If the account does not exist, we need to create it
                    Ok(true)
                }
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn generate_transfer_transaction(
        &self,
        network: Network,
        owner_account: &AccountWithAddress,
        params: StealthTransferParams,
        inputs: Vec<SubstateRequirement>,
        fee_transfer_statement: StealthTransferStatement,
        transfer_statement: StealthTransferStatement,
        accounts_to_create: &HashSet<ComponentAddress>,
    ) -> Result<UnsignedTransaction, StealthTransferApiError> {
        let revealed_input_amount = transfer_statement.inputs_statement.revealed_amount;
        let revealed_output_amount = transfer_statement.outputs_statement.revealed_output_amount;

        let transaction = Transaction::builder()
            .for_network(network.as_byte())
            .with_dry_run(params.is_dry_run)
            .with_fee_instructions_builder(|builder| {
                if fee_transfer_statement.inputs_statement.revealed_amount.is_positive() {
                    builder
                        .call_method(*owner_account.component_address(), "withdraw", args![
                            XTR,
                            fee_transfer_statement.inputs_statement.revealed_amount
                        ])
                        .put_last_instruction_output_on_workspace("fee_input_bucket")
                        .pay_fee_stealth_with_input_bucket(fee_transfer_statement, "fee_input_bucket")
                } else {
                    builder.pay_fee_stealth(fee_transfer_statement)
                }
            })
            .then(|builder| {
                // Badge if required
                match &params.badge_usage {
                    BadgeUsage::None => builder,
                    BadgeUsage::Resource(resx) => builder
                        .call_method(*owner_account.component_address(), "create_proof_for_resource", args![
                            resx
                        ])
                        .put_last_instruction_output_on_workspace("proof")
                        .add_input(*resx),
                    BadgeUsage::NonFungible(nft) => builder
                        .call_method(
                            *owner_account.component_address(),
                            "create_proof_by_non_fungible",
                            args![nft],
                        )
                        .put_last_instruction_output_on_workspace("proof")
                        .add_input(*nft.resource_address())
                        .add_input(nft.clone()),
                    BadgeUsage::AmountOfResource { amount, resource } => builder
                        .call_method(*owner_account.component_address(), "create_proof_by_amount", args![
                            resource, amount
                        ])
                        .put_last_instruction_output_on_workspace("proof")
                        .add_input(*resource),
                }
            })
            .then(|builder| {
                if revealed_input_amount.is_positive() {
                    builder
                        .call_method(owner_account.account.component_address, "withdraw", args![
                            params.resource_address,
                            revealed_input_amount
                        ])
                        .put_last_instruction_output_on_workspace("input_bucket")
                        .stealth_transfer_with_input_bucket(params.resource_address, transfer_statement, "input_bucket")
                } else {
                    builder.stealth_transfer(params.resource_address, transfer_statement)
                }
            })
            .then(|builder| {
                // revealed_to_account may be Some, but we only use it if revealed_output_amount is greater than zero.
                if revealed_output_amount.is_zero() {
                    return builder;
                }

                // If the transfer creates revealed outputs, deposit the bucket into the destination account.
                builder
                    .put_last_instruction_output_on_workspace("output_bucket")
                    .then(|builder| {
                        params.outputs.iter().enumerate().fold(builder, |builder, (i, output)| {
                            if !output.revealed_amount.is_positive() {
                                return builder;
                            }
                            let needs_to_split = params.outputs.len() > 1;

                            let dest_account =
                                derive_account_address_from_public_key(output.address.account_public_key());
                            let need_to_create_account = accounts_to_create.contains(&dest_account);
                            if needs_to_split {
                                let sub_bucket_name = format!("output-sub-bucket-{i}");
                                if need_to_create_account {
                                    builder
                                        .take_from_bucket("output_bucket", output.revealed_amount, &sub_bucket_name)
                                        .create_account_with_bucket(
                                            *output.address.account_public_key(),
                                            sub_bucket_name,
                                        )
                                } else {
                                    builder
                                        .take_from_bucket("output_bucket", output.revealed_amount, &sub_bucket_name)
                                        .call_method(dest_account, "deposit", args![Workspace(sub_bucket_name)])
                                }
                            } else if need_to_create_account {
                                builder
                                    .create_account_with_bucket(*output.address.account_public_key(), "output_bucket")
                            } else {
                                builder.call_method(dest_account, "deposit", args![Workspace("output_bucket")])
                            }
                        })
                    })
            })
            .then(|builder| {
                if params.badge_usage.is_none() {
                    builder
                } else {
                    builder.drop_all_proofs_in_workspace()
                }
            })
            .with_inputs(inputs)
            .build_unsigned_transaction();

        Ok(transaction)
    }

    fn add_unconfirmed_output_from_statement(
        &self,
        lock_id: WalletLockId,
        account: &AccountWithAddress,
        resource_address: ResourceAddress,
        output: &StealthUnspentOutput,
        value: u64,
        memo: Option<Memo>,
    ) -> Result<(), StealthTransferApiError> {
        self.outputs_api.add_output(&StealthOutputModel {
            owner_account: *account.component_address(),
            resource_address,
            commitment: output.output.commitment,
            value,
            sender_public_nonce: output.output.sender_public_nonce,
            view_only_key_id: account.view_only_key_id(),
            owner_key_id: account.owner_key_id(),
            encrypted_data: output.output.encrypted_data.clone(),
            status: OutputStatus::LockedUnconfirmed,
            memo,
            spend_condition: output.spend_condition.clone(),
            minimum_value_promise: output.output.minimum_value_promise,
            tag_byte: output.tag,
            lock_id: Some(lock_id),
            is_burnt: false,
            is_frozen: false,
            is_on_chain: false,
            is_condition_spendable: self.outputs_api.is_spendable_condition(&output.spend_condition),
        })?;
        Ok(())
    }
}
