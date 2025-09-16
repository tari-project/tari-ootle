//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::cmp;

use digest::crypto_common::rand_core::OsRng;
use log::*;
use tari_bor::{Deserialize, Serialize};
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::{ConvertFromByteType, FromByteType, ToByteType};
use tari_ootle_common_types::{optional::IsNotFoundError, SubstateRequirement};
use tari_ootle_wallet_crypto::{MaskAndValue, OotleAddress, UnblindedOutputStatement};
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, VaultId},
    types::Amount,
};
use tari_transaction::{args, Transaction};

use crate::{
    apis::{
        accounts::{AccountsApi, AccountsApiError},
        confidential_crypto::{ConfidentialCryptoApi, ConfidentialCryptoApiError},
        confidential_outputs::{ConfidentialOutputsApi, ConfidentialOutputsApiError},
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyBranch, KeyManagerApi, KeyManagerApiError},
        substate::{SubstateApiError, SubstatesApi},
    },
    models::{ConfidentialOutputModel, OutputStatus, WalletLockId},
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore},
};

const LOG_TARGET: &str = "tari::ootle::wallet_sdk::apis::confidential_transfers";

pub struct ConfidentialTransferApi<'a, TStore, TNetworkInterface> {
    key_manager_api: KeyManagerApi<'a, TStore>,
    accounts_api: AccountsApi<'a, TStore, TNetworkInterface>,
    outputs_api: ConfidentialOutputsApi<'a, TStore>,
    substate_api: SubstatesApi<'a, TStore, TNetworkInterface>,
    crypto_api: ConfidentialCryptoApi,
    config_api: ConfigApi<'a, TStore>,
}

impl<'a, TStore, TNetworkInterface> ConfidentialTransferApi<'a, TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(
        key_manager_api: KeyManagerApi<'a, TStore>,
        accounts_api: AccountsApi<'a, TStore, TNetworkInterface>,
        outputs_api: ConfidentialOutputsApi<'a, TStore>,
        substate_api: SubstatesApi<'a, TStore, TNetworkInterface>,
        crypto_api: ConfidentialCryptoApi,
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
        lock_id: WalletLockId,
        from_account: ComponentAddress,
        resource_address: ResourceAddress,
        spend_amount: Amount,
        input_selection: ConfidentialTransferInputSelection,
    ) -> Result<InputsToSpend, ConfidentialTransferApiError> {
        let src_vault = self
            .accounts_api
            .get_vault_by_resource(&from_account, &resource_address)?;

        let available_revealed_funds = src_vault.available_revealed_balance();

        match &input_selection {
            ConfidentialTransferInputSelection::ConfidentialOnly => {
                let (confidential_inputs, _) =
                    self.outputs_api
                        .lock_outputs_by_amount(lock_id, &src_vault.id, spend_amount)?;
                let confidential_inputs = self.outputs_api.resolve_output_masks(confidential_inputs)?;

                info!(
                    target: LOG_TARGET,
                    "ConfidentialOnly: Locked {} confidential inputs for transfer from {}",
                    confidential_inputs.len(),
                    src_vault.id,
                );

                Ok(InputsToSpend {
                    confidential: confidential_inputs,
                    lock_id,
                    revealed: Amount::zero(),
                })
            },
            ConfidentialTransferInputSelection::RevealedOnly => {
                if available_revealed_funds < spend_amount {
                    return Err(ConfidentialTransferApiError::InsufficientFunds);
                }

                self.outputs_api
                    .lock_vault_revealed_funds(lock_id, &src_vault.id, spend_amount)?;

                info!(
                    target: LOG_TARGET,
                    "RevealedOnly: Spending {} revealed balance for transfer from {}",
                    spend_amount,
                    src_vault.id,
                );

                Ok(InputsToSpend {
                    confidential: vec![],
                    lock_id,
                    revealed: spend_amount,
                })
            },
            ConfidentialTransferInputSelection::PreferRevealed => {
                let revealed_to_spend = cmp::min(src_vault.revealed_balance, spend_amount);
                let confidential_to_spend = spend_amount - revealed_to_spend;
                if confidential_to_spend.is_zero() {
                    info!(
                        target: LOG_TARGET,
                        "PreferRevealed: Spending {} revealed balance for transfer from {}",
                        revealed_to_spend,
                        src_vault.id,
                    );

                    self.outputs_api
                        .lock_vault_revealed_funds(lock_id, &src_vault.id, revealed_to_spend)?;

                    return Ok(InputsToSpend {
                        confidential: vec![],
                        lock_id,
                        revealed: revealed_to_spend,
                    });
                }

                let (confidential_inputs, _) =
                    self.outputs_api
                        .lock_outputs_by_amount(lock_id, &src_vault.id, confidential_to_spend)?;
                let confidential_inputs = self.outputs_api.resolve_output_masks(confidential_inputs)?;

                let total_confidential_spent = confidential_inputs.iter().map(|i| i.value).sum::<Amount>();

                self.outputs_api
                    .lock_vault_revealed_funds(lock_id, &src_vault.id, revealed_to_spend)?;

                info!(
                    target: LOG_TARGET,
                    "PreferRevealed: Locked {} confidential inputs (target: {}, spent: {}) and {} revealed for amount {} from {}",
                    confidential_inputs.len(),
                    confidential_to_spend,
                    total_confidential_spent,
                    revealed_to_spend,
                    spend_amount,
                    src_vault.id,
                );

                Ok(InputsToSpend {
                    confidential: confidential_inputs,
                    lock_id,
                    revealed: revealed_to_spend,
                })
            },
            ConfidentialTransferInputSelection::PreferConfidential => {
                let (confidential_inputs, amount_locked) =
                    self.outputs_api
                        .lock_outputs_until_partial_amount(lock_id, &src_vault.id, spend_amount)?;

                let revealed_to_spend = spend_amount
                    .saturating_sub_positive(amount_locked)
                    .unwrap_or_else(Amount::zero);

                if src_vault.revealed_balance < revealed_to_spend {
                    return Err(ConfidentialTransferApiError::InsufficientFunds);
                }

                self.outputs_api
                    .lock_vault_revealed_funds(lock_id, &src_vault.id, revealed_to_spend)?;

                let confidential_inputs = self.outputs_api.resolve_output_masks(confidential_inputs)?;

                Ok(InputsToSpend {
                    confidential: confidential_inputs,
                    lock_id,
                    revealed: revealed_to_spend,
                })
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn transfer(
        &self,
        params: ConfidentialTransferParams,
    ) -> Result<TransferOutput, ConfidentialTransferApiError> {
        let from_account = self.accounts_api.get_account_by_address(&params.from_account)?;
        let to_account = self
            .accounts_api
            .resolve_account_by_public_key(params.destination_address.account_public_key())
            .await?;

        // Determine Transaction Inputs
        let mut inputs = Vec::new();

        let dest_account_exists = to_account.exists_on_chain;
        if dest_account_exists {
            inputs.push(SubstateRequirement::unversioned(to_account.address));
            inputs.extend(to_account.vaults.into_iter().map(SubstateRequirement::unversioned))
        }

        let account = self.accounts_api.get_account_by_address(&params.from_account)?;
        let account_substate = self.substate_api.get_substate(&params.from_account.into())?;
        inputs.push(account_substate.substate_id.into_unversioned_requirement());

        // Add all versioned account child addresses as inputs
        let child_addresses = self
            .substate_api
            .load_dependent_substates(&[&account.account.component_address.into()])?;
        inputs.extend(child_addresses.into_iter().map(|a| a.into_unversioned()));

        let src_vault = self
            .accounts_api
            .get_vault_by_resource(account.component_address(), &params.resource_address)?;
        let src_vault_substate = self.substate_api.get_substate(&src_vault.id.into())?;
        inputs.push(src_vault_substate.substate_id.into_unversioned_requirement());

        // add the input for the resource address to be transferred
        inputs.push(SubstateRequirement::unversioned(params.resource_address));

        // We need to fetch the resource substate to check if there is a view key present.
        let resource = self.substate_api.fetch_resource(params.resource_address).await?;

        if let Some(ref resource_address) = params.proof_from_resource {
            inputs.push(SubstateRequirement::unversioned(*resource_address));
        }

        // Reserve and lock input funds for fees
        let max_fee = params.max_fee;

        let account_secret = self.key_manager_api.derive_account_key(account.key_index())?;
        let account_public_key = PublicKey::from_secret_key(&account_secret.key);

        // Reserve and lock input funds
        let lock_id = self.outputs_api.create_lock()?;
        let inputs_to_spend = match self.resolved_inputs_for_transfer(
            lock_id,
            params.from_account,
            params.resource_address,
            params.amount,
            params.input_selection,
        ) {
            Ok(inputs) => inputs,
            Err(e) => {
                warn!(target: LOG_TARGET, "Unlocking fee fund locks after error: {}", e);
                return Err(e);
            },
        };

        // Generate outputs
        let resource_view_key = resource
            .view_key()
            .map(RistrettoPublicKey::convert_from_byte_type)
            .transpose()
            .map_err(|e| ConfidentialTransferApiError::InvalidParameter {
                param: "resource_view_key",
                reason: format!("Invalid resource view key: {e}"),
            })?;
        let destination_pk = params
            .destination_address
            .account_public_key()
            .try_from_byte_type()
            .map_err(|e| ConfidentialTransferApiError::InvalidParameter {
                param: "destination_public_key",
                reason: format!("Invalid destination public key: {e}"),
            })?;

        let output_statement = if params.confidential_amount().is_zero() {
            None
        } else {
            Some(self.create_confidential_proof_statement(
                &destination_pk,
                params.confidential_amount(),
                resource_view_key.clone(),
            )?)
        };

        let remaining_left_to_pay = params
            .amount
            .checked_sub_positive(inputs_to_spend.revealed)
            .unwrap_or_else(|| {
                panic!(
                    "BUG: paid more revealed funds ({}) than the amount to pay ({})",
                    inputs_to_spend.revealed, params.amount
                )
            });
        let change_confidential_amount = inputs_to_spend.total_confidential_amount() - remaining_left_to_pay;

        let maybe_change_statement = if change_confidential_amount.is_positive() {
            let statement = self.create_confidential_proof_statement(
                &account_public_key,
                change_confidential_amount,
                resource_view_key,
            )?;

            let change_value = statement.amount;

            if change_value.is_positive() {
                self.outputs_api.add_output(ConfidentialOutputModel {
                    account_address: *account.component_address(),
                    vault_id: src_vault.id,
                    commitment: statement
                        .to_commitment()
                        .expect("BUG: to_commitment negative amount")
                        .to_byte_type(),
                    value: change_value,
                    sender_public_nonce: Some(statement.sender_public_nonce.to_byte_type()),
                    encryption_secret_key_index: account_secret.key_index,
                    encrypted_data: statement.encrypted_data.clone(),
                    public_asset_tag: None,
                    status: OutputStatus::LockedUnconfirmed,
                    lock_id: Some(inputs_to_spend.lock_id),
                })?;
            }

            Some(statement)
        } else {
            None
        };

        let proof = self.crypto_api.generate_withdraw_proof(
            &inputs_to_spend.confidential,
            inputs_to_spend.revealed,
            output_statement.as_ref(),
            params.revealed_amount(),
            maybe_change_statement.as_ref(),
            Amount::zero(),
        )?;

        let network = self.config_api.get_network()?;
        let transaction = Transaction::builder()
            .for_network(network.as_byte())
            .with_dry_run(params.is_dry_run)
            // TODO: we assume that from_account has XTR
            .fee_transaction_pay_from_component(*from_account.component_address(), max_fee)
            .then(|builder| {
                if dest_account_exists {
                    builder
                } else {
                    builder.create_account(*params.destination_address.account_public_key())
                }
            })
            .then(|builder| {
                if let Some(ref badge) = params.proof_from_resource {
                    builder
                        .call_method(*from_account.component_address(), "create_proof_for_resource", args![badge])
                        .put_last_instruction_output_on_workspace("proof")
                } else {
                    builder
                }
            })
            .call_method(*from_account.component_address(), "withdraw_confidential", args![
                params.resource_address,
                proof
            ])
            .put_last_instruction_output_on_workspace("bucket")
            .call_method(to_account.address, "deposit", args![Workspace("bucket")])
            .then(|builder| {
                if params.proof_from_resource.is_some() {
                    builder.drop_all_proofs_in_workspace()
                } else {
                    builder
                }
            })
            .with_inputs(inputs)
            .build_and_seal(&account_secret.key);

        let tx_id = transaction.calculate_id();
        self.outputs_api
            .locks_set_transaction_id(inputs_to_spend.lock_id, tx_id)?;

        Ok(TransferOutput {
            transaction,
            transaction_proof_id: inputs_to_spend.lock_id,
        })
    }

    fn create_confidential_proof_statement(
        &self,
        dest_public_key: &RistrettoPublicKey,
        confidential_amount: Amount,
        resource_view_key: Option<RistrettoPublicKey>,
    ) -> Result<UnblindedOutputStatement, ConfidentialTransferApiError> {
        if !confidential_amount.is_positive() {
            return Err(ConfidentialTransferApiError::InvalidParameter {
                param: "confidential_amount",
                reason: "Confidential amount must be positive".to_string(),
            });
        }

        let mask = self.key_manager_api.next_key(KeyBranch::ConfidentialMask)?;

        let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
        let encrypted_data = self.crypto_api.encrypt_value_and_mask(
            confidential_amount
                .to_u64_checked()
                .ok_or_else(|| ConfidentialTransferApiError::AmountOverflow {
                    param: "confidential_amount",
                    details: "Confidential amount exceeds u64. This is currently a limitation due to the format of \
                              EncryptedData"
                        .to_string(),
                })?,
            &mask.key,
            dest_public_key,
            &nonce,
        )?;

        Ok(UnblindedOutputStatement {
            amount: confidential_amount,
            mask: mask.key,
            sender_public_nonce: public_nonce,
            encrypted_data,
            minimum_value_promise: 0,
            resource_view_key,
        })
    }
}

pub struct TransferOutput {
    pub transaction: Transaction,
    pub transaction_proof_id: WalletLockId,
}

#[derive(Debug)]
pub struct ConfidentialTransferParams {
    /// Spend from this account
    pub from_account: ComponentAddress,
    /// Strategy for input selection
    pub input_selection: ConfidentialTransferInputSelection,
    /// Amount to spend to destination
    pub amount: Amount,
    /// Destination address used to derive the destination account component
    pub destination_address: OotleAddress,
    /// Address of the resource to transfer
    pub resource_address: ResourceAddress,
    /// Fee to lock for the transaction
    pub max_fee: u64,
    /// If true, the output will contain only a revealed amount. Otherwise, only confidential amounts.
    pub output_to_revealed: bool,
    /// If some, instructions are added that create a access rule proof for this resource before calling withdraw
    pub proof_from_resource: Option<ResourceAddress>,
    /// Run as a dry run, no funds will be transferred if true
    pub is_dry_run: bool,
}

impl ConfidentialTransferParams {
    pub fn confidential_amount(&self) -> Amount {
        if self.output_to_revealed {
            Amount::zero()
        } else {
            self.amount
        }
    }

    pub fn revealed_amount(&self) -> Amount {
        if self.output_to_revealed {
            self.amount
        } else {
            Amount::zero()
        }
    }
}

impl ConfidentialTransferParams {
    pub fn total_amount(&self) -> Amount {
        self.amount + self.max_fee.into()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ConfidentialTransferInputSelection {
    ConfidentialOnly,
    RevealedOnly,
    PreferRevealed,
    PreferConfidential,
}

#[derive(Debug)]
pub struct InputsToSpend {
    pub confidential: Vec<MaskAndValue>,
    pub lock_id: WalletLockId,
    pub revealed: Amount,
}

impl InputsToSpend {
    pub fn total_amount(&self) -> Amount {
        self.total_confidential_amount() + self.revealed
    }

    pub fn total_confidential_amount(&self) -> Amount {
        self.confidential.iter().map(|o| o.value).sum()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfidentialTransferApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Confidential crypto error: {0}")]
    ConfidentialCrypto(#[from] ConfidentialCryptoApiError),
    #[error("Confidential outputs error: {0}")]
    OutputsApi(#[from] ConfidentialOutputsApiError),
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

impl IsNotFoundError for ConfidentialTransferApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}

pub struct ResolvedAccountDetails {
    pub address: ComponentAddress,
    pub vaults: Vec<VaultId>,
    pub exists_on_chain: bool,
}
