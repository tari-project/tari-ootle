//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::cmp;

use digest::crypto_common::rand_core::OsRng;
use log::*;
use tari_bor::{Deserialize, Serialize};
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
    models::{ComponentAddress, ResourceAddress, VaultId},
    prelude::RistrettoPublicKeyBytes,
    types::Amount,
};
use tari_transaction::{args, Transaction};

use crate::{
    apis::{
        accounts::{AccountsApi, AccountsApiError},
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyBranch, KeyManagerApi, KeyManagerApiError},
        stealth_crypto::{StealthCryptoApi, StealthCryptoApiError},
        stealth_outputs::{StealthOutputsApi, StealthOutputsApiError},
        substate::{SubstateApiError, SubstatesApi},
    },
    models::{AccountWithPublicKey, OutputLockId, OutputStatus, StealthOutputModel},
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
        from_account: &AccountWithPublicKey,
        resource_address: ResourceAddress,
        spend_amount: Amount,
        input_selection: StealthTransferInputSelection,
    ) -> Result<InputsToSpend, StealthTransferApiError> {
        let lock_id = self.outputs_api.create_lock_for_resource(&resource_address)?;
        let maybe_src_vault = self
            .accounts_api
            .get_vault_by_resource(from_account.address(), &resource_address)
            .optional()?;

        let available_revealed_funds = maybe_src_vault
            .as_ref()
            .map(|v| v.available_revealed_balance())
            .unwrap_or_else(Amount::zero);

        match input_selection {
            StealthTransferInputSelection::StealthOnly => {
                let (input_models, _) = self.outputs_api.lock_outputs_by_amount(lock_id, spend_amount)?;
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
            StealthTransferInputSelection::RevealedOnly => {
                if available_revealed_funds < spend_amount {
                    return Err(StealthTransferApiError::InsufficientFunds);
                }

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
            StealthTransferInputSelection::PreferRevealed => {
                let revealed_to_spend = cmp::min(available_revealed_funds, spend_amount);
                let utxo_amount_to_spend = spend_amount - revealed_to_spend;
                if utxo_amount_to_spend.is_zero() {
                    info!(
                        target: LOG_TARGET,
                        "PreferRevealed: Spending {} revealed balance for transfer from {}",
                        revealed_to_spend,
                        maybe_src_vault.as_ref().map(|v| v.id).display()
                    );

                    self.outputs_api.lock_revealed_funds(lock_id, revealed_to_spend)?;

                    return Ok(InputsToSpend {
                        inputs: vec![],
                        lock_id,
                        revealed: revealed_to_spend,
                    });
                }

                let (inputs, _) = self.outputs_api.lock_outputs_by_amount(lock_id, utxo_amount_to_spend)?;
                let inputs = self
                    .outputs_api
                    .resolve_output_masks_for_spending(from_account, inputs)?;

                let total_confidential_spent = Amount::sum_from_positive(inputs.iter().map(|i| i.mask_and_value.value))
                    // The wallet has somehow stored a negative amount, which should not happen.
                    .expect("BUG: an unblinded input amount was negative");

                self.outputs_api.lock_revealed_funds(lock_id, revealed_to_spend)?;

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
            StealthTransferInputSelection::PreferStealth => {
                let lock_id = self.outputs_api.create_lock_for_resource(&resource_address)?;
                let (blinded_inputs, blinded_amount_locked) = self
                    .outputs_api
                    .lock_outputs_until_partial_amount(spend_amount, lock_id)?;

                let revealed_to_spend = spend_amount
                    .saturating_sub_positive(blinded_amount_locked)
                    .unwrap_or_else(Amount::zero);

                if available_revealed_funds < revealed_to_spend {
                    return Err(StealthTransferApiError::InsufficientFunds);
                }

                self.outputs_api.lock_revealed_funds(lock_id, revealed_to_spend)?;

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
        // TODO: XTR as a stealth resource
        let fee_account = self.accounts_api.get_default()?;

        let (_, owner_public_key) = self
            .key_manager_api
            .derive_account_keypair(params.owner_account.key_index())?;

        // Determine Transaction Inputs
        let mut inputs = Vec::new();

        let fee_account_substate = self.substate_api.get_substate(&fee_account.account.address.into())?;
        inputs.push(fee_account_substate.substate_id.into_unversioned_requirement());

        // Add all versioned account child addresses as inputs
        let child_addresses = self
            .substate_api
            .load_dependent_substates(&[&fee_account.account.address.into()])?;
        inputs.extend(child_addresses.into_iter().map(|a| a.into_unversioned()));

        let src_vault = self
            .accounts_api
            .get_vault_by_resource(fee_account.address(), &params.resource_address)?;
        let src_vault_substate = self.substate_api.get_substate(&src_vault.id.into())?;
        inputs.push(src_vault_substate.substate_id.into_unversioned_requirement());

        // add the input for the resource address to be transferred
        inputs.push(SubstateRequirement::unversioned(params.resource_address));

        // We need to fetch the resource substate to check if there is a view key present.
        // TODO: cache
        let resource_substate = self
            .substate_api
            .scan_for_substate(&SubstateId::Resource(params.resource_address), None)
            .await?;

        let fee_account_secret = self.key_manager_api.derive_account_key(fee_account.key_index())?;

        // Reserve and lock input funds
        // TODO: preserve atomicity across api calls - needed in many places
        let inputs_to_spend = match self.resolved_inputs_for_transfer(
            &params.owner_account,
            params.resource_address,
            params.blinded_output_amount,
            params.input_selection,
        ) {
            Ok(inputs) => inputs,
            Err(e) => {
                warn!(target: LOG_TARGET, "Unlocking fee fund locks after error: {}", e);
                // This is a hack that addresses the case where input locking fails after the fee transaction.
                // However any error after this point do not undo locking. This is a limitation
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

        // Generate outputs
        let resource_view_key = resource_substate
            .substate
            .as_resource()
            .ok_or_else(|| StealthTransferApiError::UnexpectedIndexerResponse {
                details: format!(
                    "Expected indexer to return resource for address {}. It returned {}",
                    params.resource_address, resource_substate.address
                ),
            })?
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

        let output_statement =
            self.create_output_statement(&destination_pk, params.blinded_output_amount, resource_view_key.clone())?;

        let change_confidential_amount = inputs_to_spend
            .total_stealth_input_amount()
            .checked_sub_positive(params.blinded_output_amount)
            .expect("BUG: total_stealth_input_amount or params.blinded_output_amount are negative after validation");

        let maybe_change_statement = if change_confidential_amount.is_zero() {
            None
        } else {
            let change =
                self.create_output_statement(&owner_public_key, change_confidential_amount, resource_view_key)?;

            let change_value = change.statement.amount;

            if !change.statement.amount.is_zero() {
                self.outputs_api.add_output(StealthOutputModel {
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
                    lock_id: Some(inputs_to_spend.lock_id),
                })?;
            }

            Some(change)
        };

        let outputs = Some(output_statement)
            .filter(|o| !o.statement.amount.is_zero())
            .into_iter()
            .chain(maybe_change_statement)
            .collect::<Vec<_>>();

        let transfer_statement = self.crypto_api.generate_transfer_statement(
            &inputs_to_spend.inputs,
            inputs_to_spend.revealed,
            &outputs,
            params.revealed_output_amount,
        )?;

        let network = self.config_api.get_network()?;
        let transaction = Transaction::builder()
            .for_network(network.as_byte())
            .with_dry_run(params.is_dry_run)
            // TODO: pay fees using stealth XTR when that is implemented
            .fee_transaction_pay_from_component(*fee_account.address(), params.max_fee)
            .stealth_transfer(params.resource_address, transfer_statement)
            .then(|builder| {
                // revealed_to_account may be Some, but we only use it if revealed_output_amount is greater than zero.
                if params.revealed_output_amount.is_zero() {
                    return builder;
                }

                // If the transfer creates revealed outputs, deposit the bucket into the destination account.
                if let Some(address) = params.revealed_to_account {
                    builder.put_last_instruction_output_on_workspace("bucket").call_method(
                        address,
                        "deposit",
                        args![Workspace("bucket")],
                    )
                } else {
                    builder
                }
            })
            .with_inputs(inputs)
            .build_and_seal(&fee_account_secret.key);

        let tx_id = transaction.calculate_id();
        self.outputs_api
            .set_transaction_hash_for_lock(inputs_to_spend.lock_id, tx_id)?;
        // self.outputs_api
        //     .proofs_set_transaction_hash(fee_inputs_to_spend.proof_id, tx_id)?;

        Ok(TransferOutput {
            transaction,
            // fee_transaction_proof_id: Some(fee_inputs_to_spend.proof_id),
            transaction_proof_id: Some(inputs_to_spend.lock_id),
        })
    }

    fn create_output_statement(
        &self,
        dest_public_key: &RistrettoPublicKey,
        confidential_amount: Amount,
        resource_view_key: Option<RistrettoPublicKey>,
    ) -> Result<UnblindedStealthOutputStatement, StealthTransferApiError> {
        if !confidential_amount.is_positive() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "confidential_amount",
                reason: "Confidential amount must be positive".to_string(),
            });
        }

        let mask = self.key_manager_api.next_key(KeyBranch::StealthMasks)?;

        let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
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
            &nonce,
        )?;

        Ok(UnblindedStealthOutputStatement {
            statement: UnblindedOutputStatement {
                amount: confidential_amount,
                mask: mask.key,
                sender_public_nonce: public_nonce,
                encrypted_data,
                minimum_value_promise: 0,
                resource_view_key,
            },
            output_owner_public_key: dest_public_key.clone(),
        })
    }
}

pub struct TransferOutput {
    pub transaction: Transaction,
    // pub fee_transaction_proof_id: Option<OutputLockId>,
    pub transaction_proof_id: Option<OutputLockId>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum StealthTransferInputSelection {
    StealthOnly,
    RevealedOnly,
    PreferRevealed,
    PreferStealth,
}

#[derive(Debug)]
pub struct StealthTransferParams {
    /// Address of the owner account. This determines used to derive
    pub owner_account: AccountWithPublicKey,
    /// Address of the account to transfer revealed funds. This must be Some if `revealed_output_amount` is greater
    /// than zero.
    pub revealed_to_account: Option<ComponentAddress>,
    /// Strategy for input selection
    pub input_selection: StealthTransferInputSelection,
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

        if self.revealed_output_amount.is_positive() && self.revealed_to_account.is_none() {
            return Err(StealthTransferApiError::InvalidParameter {
                param: "revealed_to_account",
                reason: "revealed_to_account must be Some if revealed_output_amount is greater than zero".to_string(),
            });
        }

        Ok(())
    }

    pub fn total_amount(&self) -> Amount {
        self.blinded_output_amount + self.revealed_output_amount
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
