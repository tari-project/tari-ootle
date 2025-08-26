//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::cmp;

use digest::crypto_common::rand_core::OsRng;
use log::*;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::{substate::SubstateId, FromByteType, ToByteType, UtxoAddress};
use tari_ootle_common_types::{
    displayable::Displayable,
    optional::{IsNotFoundError, Optional},
    SubstateRequirement,
};
use tari_ootle_wallet_crypto::{
    MaskAndValue,
    UnblindedOutputStatement,
    UnblindedStealthInputStatement,
    UnblindedStealthOutputStatement,
};
use tari_template_lib::{
    models::{Account as BuiltinAccount, ComponentAddress, ResourceAddress, VaultId},
    prelude::{RistrettoPublicKeyBytes, XTR},
    types::Amount,
};
use tari_transaction::{args, Transaction};

use crate::{
    apis::{
        accounts::{derive_account_address_from_public_key, AccountsApi, AccountsApiError},
        confidential_transfer::ConfidentialTransferInputSelection,
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyBranch, KeyManagerApi, KeyManagerApiError},
        stealth_crypto::{StealthCryptoApi, StealthCryptoApiError},
        stealth_outputs::{StealthOutputsApi, StealthOutputsApiError},
        substate::{SubstateApiError, SubstatesApi, ValidatorScanResult},
    },
    models::{Account, AccountWithPublicKey, OutputLockId, OutputStatus, StealthOutputModel},
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore},
};

const LOG_TARGET: &str = "tari::ootle::wallet_sdk::apis::stealth_transfers";

pub struct StealthTransferApi<'a, TStore, TNetworkInterface> {
    key_manager_api: KeyManagerApi<'a, TStore>,
    accounts_api: AccountsApi<'a, TStore, TNetworkInterface>,
    outputs_api: StealthOutputsApi<'a, TStore>,
    substate_api: SubstatesApi<'a, TStore, TNetworkInterface>,
    crypto_api: StealthCryptoApi,
    config_api: ConfigApi<'a, TStore>,
}

impl<'a, TStore, TNetworkInterface> StealthTransferApi<'a, TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(
        key_manager_api: KeyManagerApi<'a, TStore>,
        accounts_api: AccountsApi<'a, TStore, TNetworkInterface>,
        outputs_api: StealthOutputsApi<'a, TStore>,
        substate_api: SubstatesApi<'a, TStore, TNetworkInterface>,
        crypto_api: StealthCryptoApi,
        config_api: ConfigApi<'a, TStore>,
    ) -> Self {
        Self {
            key_manager_api,
            accounts_api,
            outputs_api,
            substate_api,
            crypto_api,
            config_api,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn resolved_inputs_for_transfer(
        &self,
        from_account: &Account,
        resource_address: ResourceAddress,
        spend_amount: Amount,
        input_selection: ConfidentialTransferInputSelection,
    ) -> Result<InputsToSpend, StealthTransferApiError> {
        if !spend_amount.is_positive() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "spend_amount",
                reason: "Spend amount must be positive".to_string(),
            });
        }

        let maybe_src_vault = self
            .accounts_api
            .get_vault_by_resource(from_account.address(), &resource_address)
            .optional()?;

        let available_revealed_funds = maybe_src_vault
            .as_ref()
            .map(|v| v.available_revealed_balance())
            .unwrap_or_else(Amount::zero);

        match input_selection {
            ConfidentialTransferInputSelection::ConfidentialOnly => {
                let lock_id = self.outputs_api.create_lock_for_resource(&resource_address)?;
                let (input_models, _) = self.outputs_api.lock_outputs_in_account_by_amount(
                    from_account.address(),
                    lock_id,
                    spend_amount,
                )?;
                let inputs = self
                    .outputs_api
                    .resolve_output_masks_for_spending(from_account, input_models)?;

                info!(
                    target: LOG_TARGET,
                    "ConfidentialOnly: Locked {} confidential inputs for transfer",
                    inputs.len(),
                );

                Ok(InputsToSpend {
                    inputs,
                    lock_id,
                    revealed: Amount::zero(),
                })
            },
            ConfidentialTransferInputSelection::RevealedOnly => {
                if available_revealed_funds < spend_amount {
                    return Err(StealthTransferApiError::InsufficientFunds);
                }

                let src_vault =
                    maybe_src_vault
                        .as_ref()
                        .ok_or_else(|| StealthTransferApiError::InsufficientRevealedFunds {
                            details: format!(
                                "No vault found for resource {} in account {}",
                                resource_address,
                                from_account.address()
                            ),
                        })?;

                let lock_id = self.outputs_api.create_lock_for_vault(&src_vault.id)?;
                self.outputs_api.lock_revealed_funds(lock_id, spend_amount)?;

                info!(
                    target: LOG_TARGET,
                    "RevealedOnly: Spending {} revealed balance for transfer from {}",
                    spend_amount,
                    maybe_src_vault.as_ref().map(|v| v.id).display()
                );

                Ok(InputsToSpend {
                    inputs: vec![],
                    lock_id,
                    revealed: spend_amount,
                })
            },
            ConfidentialTransferInputSelection::PreferRevealed => {
                let maybe_vault_lock_id = maybe_src_vault
                    .as_ref()
                    .map(|v| self.outputs_api.create_lock_for_vault(&v.id))
                    .transpose()?;
                let revealed_to_spend = cmp::min(available_revealed_funds, spend_amount);
                let utxo_amount_to_spend = spend_amount - revealed_to_spend;
                if let Some(lock_id) = maybe_vault_lock_id {
                    if utxo_amount_to_spend.is_zero() {
                        info!(
                            target: LOG_TARGET,
                            "PreferRevealed: Spending {} revealed balance (available: {}) for transfer from {}",
                            revealed_to_spend,
                            available_revealed_funds,
                            maybe_src_vault.as_ref().map(|v| v.id).display()
                        );

                        self.outputs_api.lock_revealed_funds(lock_id, revealed_to_spend)
                            .inspect_err(|_| {
                                // TODO: atomic rollback will help with this
                                if let Err(err) = self.outputs_api.release_locked_outputs(lock_id) {
                                    error!(target: LOG_TARGET, "Failed to release lock outputs for resource {}: {}", resource_address, err);
                                }
                            })?;

                        return Ok(InputsToSpend {
                            inputs: vec![],
                            lock_id,
                            revealed: revealed_to_spend,
                        });
                    }
                }

                let lock_id = match maybe_vault_lock_id {
                    Some(lock_id) => lock_id,
                    None => {
                        if revealed_to_spend.is_positive() {
                            // No vault containing revealed funds was found
                            return Err(StealthTransferApiError::InsufficientRevealedFunds {
                                details: format!(
                                    "PreferRevealed: No vault found for resource {} in account {}. Need to spend {} \
                                     revealed funds",
                                    resource_address,
                                    from_account.address(),
                                    revealed_to_spend
                                ),
                            });
                        }
                        self.outputs_api.create_lock_for_resource(&resource_address)?
                    },
                };

                let (inputs, _) = self.outputs_api.lock_outputs_in_account_by_amount(from_account.address(), lock_id, utxo_amount_to_spend)
                    .inspect_err(|_| {
                        // TODO: atomic rollback will help with this
                        if let Err(err) = self.outputs_api.release_locked_outputs(lock_id) {
                            error!(target: LOG_TARGET, "Failed to release lock outputs for resource {}: {}", resource_address, err);
                        }
                    })?;
                let inputs = self
                    .outputs_api
                    .resolve_output_masks_for_spending(from_account, inputs)
                    .inspect_err(|_| {
                        // TODO: atomic rollback will help with this
                        if let Err(err) = self.outputs_api.release_locked_outputs(lock_id) {
                            error!(target: LOG_TARGET, "Failed to release lock outputs for resource {}: {}", resource_address, err);
                        }
                    })?;

                let total_confidential_spent = Amount::sum_from_positive(inputs.iter().map(|i| i.mask_and_value.value))
                    // The wallet has somehow stored a negative amount, which should not happen.
                    .expect("BUG: an unblinded input amount was negative");

                if revealed_to_spend.is_positive() {
                    self.outputs_api.lock_revealed_funds(lock_id, revealed_to_spend)
                        .inspect_err(|_| {
                            // TODO: atomic rollback will help with this
                            if let Err(err) = self.outputs_api.release_locked_outputs(lock_id) {
                                error!(target: LOG_TARGET, "Failed to release lock outputs for resource {}: {}", resource_address, err);
                            }
                        })?;
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
                    lock_id,
                    revealed: revealed_to_spend,
                })
            },
            ConfidentialTransferInputSelection::PreferConfidential => {
                let lock_id = self.outputs_api.create_lock_for_resource(&resource_address)?;
                let (blinded_inputs, blinded_amount_locked) = self.outputs_api.lock_outputs_until_partial_amount(
                    from_account.address(),
                    spend_amount,
                    lock_id,
                )?;

                let revealed_to_spend = spend_amount
                    .saturating_sub_positive(blinded_amount_locked)
                    .unwrap_or_else(Amount::zero);

                if available_revealed_funds < revealed_to_spend {
                    self.outputs_api.release_locked_outputs(lock_id)?;
                    return Err(StealthTransferApiError::InsufficientFunds);
                }

                if revealed_to_spend.is_positive() {
                    match maybe_src_vault {
                        Some(vault) => {
                            self.outputs_api.set_vault_id_for_lock(lock_id, vault.id)?;
                            self.outputs_api.lock_revealed_funds(lock_id, revealed_to_spend)?;
                        },
                        None => {
                            if let Err(err) = self.outputs_api.release_locked_outputs(lock_id) {
                                error!(target: LOG_TARGET, "🚨 Failed to release lock outputs for resource {}: {}", resource_address, err);
                            }
                            return Err(StealthTransferApiError::InsufficientRevealedFunds {
                                details: format!(
                                    "PreferConfidential: No vault found for resource {} in account {}. Need to spend \
                                     {} revealed funds",
                                    resource_address,
                                    from_account.address(),
                                    revealed_to_spend
                                ),
                            });
                        },
                    }
                }

                let inputs = self
                    .outputs_api
                    .resolve_output_masks_for_spending(from_account, blinded_inputs)?;

                Ok(InputsToSpend {
                    inputs,
                    lock_id,
                    revealed: revealed_to_spend,
                })
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn transfer(&self, params: StealthTransferParams) -> Result<TransferOutput, StealthTransferApiError> {
        params.validate()?;

        let destination_account = derive_account_address_from_public_key(&params.destination_public_key);

        // Determine Transaction Inputs
        let mut inputs = Vec::new();
        let fee_account = params.owner_account.clone();

        inputs.push(SubstateRequirement::unversioned(*fee_account.address()));

        // Add all versioned account child addresses as inputs
        let child_addresses = self
            .substate_api
            .load_dependent_substates(&[&(*fee_account.address()).into()])?;
        inputs.extend(child_addresses.into_iter().map(|a| a.into_unversioned()));

        let fee_vault = self
            .accounts_api
            .get_vault_by_resource(fee_account.address(), &XTR)
            .optional()?
            .ok_or_else(|| StealthTransferApiError::InvalidParameter {
                param: "fee_resource",
                reason: format!(
                    "XTR fee vault not found for fee account {}. Create or fund an XTR vault to pay fees.",
                    fee_account.address()
                ),
            })?;
        inputs.push(SubstateRequirement::unversioned(fee_vault.id));

        // add the input for the resource address to be transferred
        inputs.push(SubstateRequirement::unversioned(params.resource_address));

        let need_to_create_dest_account = if params.revealed_output_amount.is_positive() {
            match self
                .accounts_api
                .get_account_by_address(&destination_account)
                .optional()?
            {
                Some(local_account) => {
                    if local_account.is_confirmed_on_chain() {
                        inputs.push(SubstateRequirement::unversioned(destination_account));
                        if let Some(vault) = self
                            .accounts_api
                            .get_vault_by_resource(local_account.address(), &params.resource_address)
                            .optional()?
                        {
                            inputs.push(SubstateRequirement::unversioned(vault.id));
                        }

                        false
                    } else {
                        true
                    }
                },
                None => {
                    // TODO: we're just determining if the account exists - symptom of a larger problem/missing
                    // feature where account is created as needed by the execution layer instead of having to be
                    // determined by the client side
                    let to_account_substate = self
                        .substate_api
                        .fetch_substate_from_network(&SubstateId::Component(destination_account), None)
                        .await
                        .optional()?;

                    if let Some(ValidatorScanResult { id: address, substate }) = to_account_substate {
                        inputs.push(SubstateRequirement::unversioned(destination_account));

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
                        if let Some(vault) = dest_account.get_vault_by_resource(&params.resource_address) {
                            debug!(
                                target: LOG_TARGET,
                                "Found existing vault {} for resource {} in destination account {}",
                                vault.vault_id(),
                                params.resource_address,
                                destination_account
                            );
                            inputs.push(SubstateRequirement::unversioned(vault.vault_id()));
                        } else {
                            debug!(
                                target: LOG_TARGET,
                                "No existing vault found for resource {} in destination account {}. It will be created.",
                                params.resource_address,
                                destination_account
                            );
                        }
                        false
                    } else {
                        // If the account does not exist, we need to create it
                        true
                    }
                },
            }
        } else {
            false
        };

        // We need to fetch the resource substate to check if there is a view key present.
        let resource = self.substate_api.fetch_resource(params.resource_address).await?;

        // Generate outputs
        let resource_view_key = resource
            .view_key()
            .map(RistrettoPublicKey::try_from_byte_type)
            .transpose()
            .map_err(|e| StealthTransferApiError::InvalidParameter {
                param: "resource_view_key",
                reason: format!("Invalid resource view key: {e}"),
            })?;
        let destination_pk = RistrettoPublicKey::try_from_byte_type(&params.destination_public_key).map_err(|e| {
            StealthTransferApiError::InvalidParameter {
                param: "destination_public_key",
                reason: format!("Invalid destination public key: {e}"),
            }
        })?;

        let output_statement = params
            .blinded_output_amount
            .is_positive()
            .then(|| {
                self.create_output_statement(&destination_pk, params.blinded_output_amount, resource_view_key.clone())
            })
            .transpose()?;

        // Reserve and lock input funds
        // TODO: preserve atomicity across api calls - needed in many places
        let inputs_to_spend = match self.resolved_inputs_for_transfer(
            params.owner_account.account(),
            params.resource_address,
            params.total_output_amount(),
            params.input_selection,
        ) {
            Ok(inputs) => inputs,
            Err(e) => {
                warn!(target: LOG_TARGET, "Unlocking fee fund locks after error: {}", e);
                // This is a hack that addresses the case where input locking fails after the fee transaction.
                // However, any error after this point do not undo locking. This is a limitation
                // of the current design - the db transaction should be passed in and
                // automatically rolled back on error.
                // if let Err(err) = self.outputs_api.release_proof_outputs(fee_inputs_to_spend.proof_id) {
                //     error!(
                //         target: LOG_TARGET,
                //         "Failed to release fee inputs for transfer: {}",
                //         err
                //     );
                // }

                return Err(e);
            },
        };

        // Add all input UTXO substates to transaction inputs
        inputs.extend(
            inputs_to_spend
                .inputs
                .iter()
                .map(|i| SubstateRequirement::unversioned(to_utxo_address(params.resource_address, &i.mask_and_value))),
        );

        let result = self.generate_transfer_transaction(
            params,
            fee_account,
            inputs,
            resource_view_key,
            output_statement,
            &inputs_to_spend,
            need_to_create_dest_account,
        );

        match result {
            Ok(transaction) => Ok(TransferOutput {
                transaction,
                transaction_lock_id: Some(inputs_to_spend.lock_id),
            }),
            Err(err) => {
                // Unlock inputs
                if let Err(e) = self.outputs_api.release_locked_outputs(inputs_to_spend.lock_id) {
                    error!(target: LOG_TARGET, "Failed to release inputs lock after error: {}", e);
                }
                Err(err)
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn generate_transfer_transaction(
        &self,
        params: StealthTransferParams,
        fee_account: AccountWithPublicKey,
        inputs: Vec<SubstateRequirement>,
        resource_view_key: Option<RistrettoPublicKey>,
        maybe_output_statement: Option<UnblindedStealthOutputStatement>,
        inputs_to_spend: &InputsToSpend,
        need_to_create_account: bool,
    ) -> Result<Transaction, StealthTransferApiError> {
        let change_amount = inputs_to_spend
            .total_amount()
            .checked_sub_positive(params.total_output_amount())
            .unwrap_or_else(|| {
                // This is a bug because the wallet chooses inputs based on the required outputs. This function should
                // not have been called if there are insufficient funds.
                error!(
                    target: LOG_TARGET,
                    "BUG: total_stealth_input_amount or params.total_amount() are negative after validation"
                );
                panic!("BUG: total_stealth_input_amount or params.total_amount() are negative after validation");
            });

        let maybe_change_statement = if change_amount.is_zero() {
            None
        } else {
            let change_public_key = RistrettoPublicKey::try_from_byte_type(&params.owner_account.owner_public_key)
                .map_err(|e| StealthTransferApiError::InvalidParameter {
                    param: "owner_public_key",
                    reason: format!("Invalid owner public key: {e}"),
                })?;

            let change = self.create_output_statement(&change_public_key, change_amount, resource_view_key)?;

            let change_value = change.statement.amount;

            if !change.statement.amount.is_zero() {
                self.outputs_api.add_output(&StealthOutputModel {
                    owner_account: *params.owner_account.address(),
                    resource_address: params.resource_address,
                    commitment: change
                        .statement
                        .to_commitment()
                        .expect("BUG: to_commitment negative amount")
                        .to_byte_type(),
                    value: change_value,
                    sender_public_nonce: change.statement.sender_public_nonce.to_byte_type(),
                    encryption_secret_key_index: params.owner_account.key_index(),
                    encrypted_data: change.statement.encrypted_data.clone(),
                    status: OutputStatus::LockedUnconfirmed,
                    tag_byte: change.tag,
                    lock_id: Some(inputs_to_spend.lock_id),
                    is_burnt: false,
                    is_frozen: false,
                })?;
            }

            Some(change)
        };

        let outputs = maybe_output_statement
            .filter(|o| o.statement.amount.is_positive())
            .into_iter()
            .chain(maybe_change_statement)
            .collect::<Vec<_>>();

        let transfer_statement = self.crypto_api.generate_transfer_statement(
            &inputs_to_spend.inputs,
            inputs_to_spend.revealed,
            &outputs,
            params.revealed_output_amount,
        )?;

        let fee_account_secret = self.key_manager_api.derive_account_key(fee_account.key_index())?;
        let src_account_secret = self
            .key_manager_api
            .derive_account_key(params.owner_account.key_index())?;

        let network = self.config_api.get_network()?;
        let transaction = Transaction::builder()
            .for_network(network.as_byte())
            .with_dry_run(params.is_dry_run)
            // TODO: pay fees using stealth XTR when that is implemented
            .fee_transaction_pay_from_component(*fee_account.address(), params.max_fee)
            .then(|builder| {
                if inputs_to_spend.revealed.is_positive() {
                    builder
                        .call_method(params.owner_account.account.address, "withdraw", args![params.resource_address, inputs_to_spend.revealed])
                        .put_last_instruction_output_on_workspace("input_bucket")
                        .stealth_transfer_with_input_bucket(params.resource_address, transfer_statement, "input_bucket")
                } else {
                    builder
                        .stealth_transfer(params.resource_address, transfer_statement)
                }
            })
            .then(|builder| {
                // revealed_to_account may be Some, but we only use it if revealed_output_amount is greater than zero.
                if params.revealed_output_amount.is_zero() {
                    return builder;
                }

                // If the transfer creates revealed outputs, deposit the bucket into the destination account.
                builder.put_last_instruction_output_on_workspace("output_bucket").then(|builder| {
                    if need_to_create_account {
                        builder.create_account_with_bucket(
                            params.destination_public_key,
                            "output_bucket",
                        )
                    } else {
                        builder.call_method(
                            params.derived_destination_account(),
                            "deposit",
                            args![Workspace("output_bucket")],
                        )
                    }
                })
            })
            .with_inputs(inputs)
            .then(|builder| {
                // If the fee account is different from the owner account, we need to add a signature to authorize the revealed funds withdraw.
                if inputs_to_spend.revealed.is_positive() && params.owner_account.owner_public_key != *fee_account.owner_public_key() {
                    builder.add_signature(fee_account.owner_public_key(), &src_account_secret.key)
                } else {
                    builder
                }
            })
            .build_and_seal(&fee_account_secret.key);

        let tx_id = transaction.calculate_id();
        self.outputs_api
            .set_transaction_hash_for_lock(inputs_to_spend.lock_id, tx_id)?;
        Ok(transaction)
    }

    fn create_output_statement(
        &self,
        dest_public_key: &RistrettoPublicKey,
        confidential_amount: Amount,
        resource_view_key: Option<RistrettoPublicKey>,
    ) -> Result<UnblindedStealthOutputStatement, StealthTransferApiError> {
        let network = self.config_api.get_network()?;
        if !confidential_amount.is_positive() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "confidential_amount",
                reason: "Confidential amount must be positive".to_string(),
            });
        }

        let mask = self.key_manager_api.next_key(KeyBranch::StealthMasks)?;

        let (nonce_secret, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
        let encrypted_data = self.crypto_api.encrypt_value_and_mask(
            confidential_amount
                .to_u64_checked()
                .ok_or_else(|| StealthTransferApiError::AmountOverflow {
                    param: "confidential_amount",
                    details: "Confidential amount exceeds u64. This is currently a limitation due to the format of \
                              EncryptedData"
                        .to_string(),
                })?,
            &mask.key,
            dest_public_key,
            &nonce_secret,
        )?;

        // Create stealth address - used during spend time
        let output_owner_public_key =
            self.crypto_api
                .derive_stealth_owner_public_key(network, dest_public_key, &nonce_secret);

        let derived_tag = self
            .crypto_api
            .derive_stealth_output_tag(network, &dest_public_key.to_byte_type());

        Ok(UnblindedStealthOutputStatement {
            statement: UnblindedOutputStatement {
                amount: confidential_amount,
                mask: mask.key,
                sender_public_nonce: public_nonce,
                encrypted_data,
                minimum_value_promise: 0,
                resource_view_key,
            },
            output_owner_public_key,
            tag: derived_tag,
        })
    }
}

pub struct TransferOutput {
    pub transaction: Transaction,
    pub transaction_lock_id: Option<OutputLockId>,
}

#[derive(Debug)]
pub struct StealthTransferParams {
    /// Address of the owner account. This determines used to derive
    pub owner_account: AccountWithPublicKey,
    /// Strategy for input selection
    pub input_selection: ConfidentialTransferInputSelection,
    /// Amount of the inputs to spend to a blinded output
    pub blinded_output_amount: Amount,
    /// Amount of the inputs to spend to a revealed output
    pub revealed_output_amount: Amount,
    /// Destination public key used to derive the destination account component
    pub destination_public_key: RistrettoPublicKeyBytes,
    /// Address of the resource to transfer
    pub resource_address: ResourceAddress,
    /// Fee to lock for the transaction
    pub max_fee: u64,
    /// Run as a dry run, no funds will be transferred if true
    pub is_dry_run: bool,
}

impl StealthTransferParams {
    pub fn validate(&self) -> Result<(), StealthTransferApiError> {
        if self.blinded_output_amount.is_negative() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "blinded_output_amount",
                reason: "Blinded output amount must be non-negative".to_string(),
            });
        }

        if self.revealed_output_amount.is_negative() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "revealed_output_amount",
                reason: "Revealed output amount must be non-negative".to_string(),
            });
        }

        if self.blinded_output_amount.is_zero() && self.revealed_output_amount.is_zero() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "blinded_output_amount and revealed_output_amount",
                reason: "At least one of the amounts must be greater than zero".to_string(),
            });
        }

        Ok(())
    }

    pub fn total_output_amount(&self) -> Amount {
        self.blinded_output_amount + self.revealed_output_amount
    }

    pub fn derived_destination_account(&self) -> ComponentAddress {
        derive_account_address_from_public_key(&self.destination_public_key)
    }
}

#[derive(Debug)]
pub struct InputsToSpend {
    pub inputs: Vec<UnblindedStealthInputStatement>,
    pub lock_id: OutputLockId,
    pub revealed: Amount,
}

impl InputsToSpend {
    pub fn total_amount(&self) -> Amount {
        self.total_stealth_input_amount() + self.revealed
    }

    pub fn total_stealth_input_amount(&self) -> Amount {
        self.inputs.iter().map(|i| i.mask_and_value.value).sum()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StealthTransferApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Confidential crypto error: {0}")]
    Crypto(#[from] StealthCryptoApiError),
    #[error("Stealth outputs error: {0}")]
    OutputsApi(#[from] StealthOutputsApiError),
    #[error("Substate API error: {0}")]
    SubstateApi(#[from] SubstateApiError),
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Key manager error: {0}")]
    KeyManager(#[from] KeyManagerApiError),
    #[error("Accounts API error: {0}")]
    Accounts(#[from] AccountsApiError),
    #[error("Invalid parameter `{param}`: {reason}")]
    InvalidParameter { param: &'static str, reason: String },
    #[error("Unexpected indexer response: {details}")]
    UnexpectedIndexerResponse { details: String },
    #[error("Config API error: {0}")]
    ConfigApi(#[from] ConfigApiError),
    #[error("Amount overflow for parameter `{param}`: {details}")]
    AmountOverflow { param: &'static str, details: String },
    #[error("Insufficient revealed funds: {details}")]
    InsufficientRevealedFunds { details: String },
}

impl IsNotFoundError for StealthTransferApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}

pub struct AccountDetails {
    pub address: ComponentAddress,
    pub vaults: Vec<VaultId>,
    pub exists: bool,
}

fn to_utxo_address(resource_address: ResourceAddress, mask_and_value: &MaskAndValue) -> UtxoAddress {
    UtxoAddress::new(
        resource_address,
        mask_and_value
            .to_commitment()
            .expect("BUG: value not u64")
            .to_byte_type()
            .into(),
    )
}
