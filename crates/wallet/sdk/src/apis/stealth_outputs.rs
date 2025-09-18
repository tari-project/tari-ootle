//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey},
};
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    FromByteType,
    ToByteType,
    Utxo,
    UtxoAddress,
    UtxoOutput,
};
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
};
use tari_ootle_wallet_crypto::UnblindedStealthInputStatement;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, VaultId},
    prelude::PedersenCommitmentBytes,
    types::Amount,
};
use tari_transaction::TransactionId;

use crate::{
    apis::{
        accounts::AccountsApiError,
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyBranch, KeyManagerApi, KeyManagerApiError},
        stealth_crypto::{StealthCryptoApi, StealthCryptoApiError},
        stealth_transfer::InputToSpend,
    },
    models::{Account, KeyPair, OutputStatus, StealthBalance, StealthOutputModel, WalletLockId},
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

const LOG_TARGET: &str = "tari::ootle::wallet::apis::stealth_outputs";

pub struct StealthOutputsApi<'a, TStore> {
    store: &'a TStore,
    key_manager_api: KeyManagerApi<'a, TStore>,
    crypto_api: StealthCryptoApi,
    config_api: ConfigApi<'a, TStore>,
}

impl<'a, TStore: WalletStore> StealthOutputsApi<'a, TStore> {
    pub fn new(
        store: &'a TStore,
        key_manager_api: KeyManagerApi<'a, TStore>,
        crypto_api: StealthCryptoApi,
        config_api: ConfigApi<'a, TStore>,
    ) -> Self {
        Self {
            store,
            key_manager_api,
            crypto_api,
            config_api,
        }
    }

    /// Locks as many outputs required to reach at least the specified amount. If there are insufficient funds, an
    /// `InsufficientFunds` error is returned and no outputs are locked.
    pub fn lock_outputs_for_at_least_amount<A: Into<Amount>>(
        &self,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        lock_id: WalletLockId,
        amount: A,
    ) -> Result<(Vec<StealthOutputModel>, Amount), StealthOutputsApiError> {
        let amount = amount
            .into()
            .non_negative_checked()
            .ok_or_else(|| StealthOutputsApiError::InvalidParameter {
                param: "amount",
                reason: "Amount must be non-negative".to_string(),
            })?;
        self.store.with_write_tx(|tx| {
            let (outputs, total_output_amount) =
                self.lock_outputs_internal(tx, account_address, resource_address, amount, lock_id)?;

            if total_output_amount < amount {
                return Err(StealthOutputsApiError::InsufficientFunds);
            }

            Ok((outputs, total_output_amount))
        })
    }

    pub fn locks_set_transaction_id(
        &self,
        lock_id: WalletLockId,
        transaction_id: TransactionId,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.locks_link_transaction(lock_id, transaction_id))?;
        Ok(())
    }

    /// Locks as many outputs required to reach at least the specified amount. If there are insufficient funds, all
    /// available outputs will be locked and returned along with the total amount locked.
    pub fn lock_outputs_until_partial_amount(
        &self,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        amount: Amount,
        locked_by_id: WalletLockId,
    ) -> Result<(Vec<StealthOutputModel>, Amount), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| self.lock_outputs_internal(tx, account_address, resource_address, amount, locked_by_id))
    }

    fn lock_outputs_internal<TTx: WalletStoreWriter>(
        &self,
        tx: &mut TTx,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        amount: Amount,
        locked_by_id: WalletLockId,
    ) -> Result<(Vec<StealthOutputModel>, Amount), StealthOutputsApiError> {
        if amount.is_negative() {
            return Err(StealthOutputsApiError::InvalidParameter {
                param: "amount",
                reason: "Amount cannot be negative".to_string(),
            });
        }
        let mut total_output_amount = Amount::zero();
        let mut outputs = Vec::new();
        while total_output_amount < amount {
            let output = tx
                .stealth_outputs_lock_smallest_amount(account_address, resource_address, locked_by_id)
                .optional()?;
            match output {
                Some(output) => {
                    total_output_amount += output.value;
                    outputs.push(output);
                },
                None => {
                    debug!(
                        target: LOG_TARGET,
                        "No more outputs available to lock. Total locked amount: {}, required amount: {}",
                        total_output_amount,
                        amount
                    );
                    break;
                },
            }
        }

        Ok((outputs, total_output_amount))
    }

    pub fn add_output(&self, output: &StealthOutputModel) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.stealth_outputs_insert(output)?;
        tx.commit()?;
        Ok(())
    }

    pub fn lock_funds_in_vault<A: Into<Amount>>(
        &self,
        lock_id: WalletLockId,
        vault_id: &VaultId,
        amount_to_lock: A,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.vaults_lock_revealed_funds(lock_id, vault_id, amount_to_lock.into()))?;
        Ok(())
    }

    pub fn create_lock(&self) -> Result<WalletLockId, StealthOutputsApiError> {
        let lock_id = self.store.with_write_tx(|tx| tx.locks_create())?;
        Ok(lock_id)
    }

    pub fn release_locked_outputs(&self, lock_id: WalletLockId) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            tx.outputs_release_by_lock_id(lock_id)?;
            tx.locks_delete(lock_id)?;
            Ok(())
        })
    }

    pub fn finalize_outputs(&self, lock_id: WalletLockId) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            tx.stealth_outputs_finalize_by_lock_id(lock_id)?;
            tx.locks_delete(lock_id)?;
            Ok(())
        })
    }

    pub fn resolve_output_masks_for_spending(
        &self,
        owner_account: &Account,
        outputs: Vec<StealthOutputModel>,
    ) -> Result<Vec<InputToSpend>, StealthOutputsApiError> {
        let network = self.config_api.get_network()?;
        // Derive owner secret - the sender does not know the owner secret
        let owner_key_part = self.key_manager_api.derive_account_key(owner_account.key_index())?;
        // Derive the view-only secret, of which the public key is used by senders to encrypt the value and mask.
        let view_only = self.key_manager_api.derive_view_only_key(owner_account.key_index())?;
        let mut inputs_with_masks = Vec::with_capacity(outputs.len());
        for output in outputs {
            // Derive the decryption key from the DHKE(sender's public nonce, encryption secret key);
            let nonce = output.sender_public_nonce.try_from_byte_type().map_err(|e| {
                StealthOutputsApiError::InvalidParameter {
                    param: "sender_public_nonce",
                    reason: format!("Sender public nonce bytes are not a canonical public key: {e}"),
                }
            })?;

            let mask_and_value = self.crypto_api.decrypt_value_and_mask(
                &output.encrypted_data,
                &output.commitment,
                &view_only.key,
                &nonce,
            )?;

            let stealth_secret = self
                .crypto_api
                .derive_stealth_owner_secret(network, &owner_key_part.key, &nonce);

            inputs_with_masks.push(InputToSpend {
                statement: UnblindedStealthInputStatement {
                    mask_and_value,
                    owner_secret: stealth_secret,
                    public_nonce: nonce,
                },
                is_on_chain: output.is_on_chain,
            });
        }
        Ok(inputs_with_masks)
    }

    pub fn lock_revealed_funds<A: Into<Amount>>(
        &self,
        lock_id: WalletLockId,
        vault_id: &VaultId,
        amount_to_lock: A,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.vaults_lock_revealed_funds(lock_id, vault_id, amount_to_lock.into()))?;

        Ok(())
    }

    pub fn finalize_locked_revealed_funds(&self, lock_id: WalletLockId) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.vaults_finalized_locked_revealed_funds(lock_id)?;
        tx.commit()?;

        Ok(())
    }

    pub fn release_revealed_funds(&self, lock_id: WalletLockId) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.vaults_release_lock_revealed_funds(lock_id)?;
        tx.commit()?;

        Ok(())
    }

    pub fn get_unspent_outputs_by_account(
        &self,
        account_address: &ComponentAddress,
    ) -> Result<Vec<StealthOutputModel>, StealthOutputsApiError> {
        let balance = self
            .store
            .with_read_tx(|tx| tx.stealth_outputs_get_unspent_by_account(account_address))?;
        Ok(balance)
    }

    pub fn get_unspent_balance(
        &self,
        resource_address: &ResourceAddress,
    ) -> Result<StealthBalance, StealthOutputsApiError> {
        let balance = self
            .store
            .with_read_tx(|tx| tx.stealth_outputs_get_unspent_balance(resource_address))?;
        Ok(balance)
    }

    pub fn upsert_utxo(&self, utxo: &StealthOutputModel) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            // TODO(perf): consider a dedicated exists query
            let exists = tx
                .stealth_outputs_get_by_commitment(&utxo.resource_address, &utxo.commitment)
                .optional()?
                .is_some();
            if exists {
                let address = utxo.to_utxo_address();
                tx.stealth_outputs_update(&address, Some(utxo.is_burnt), Some(utxo.status), Some(utxo.is_frozen))
            } else {
                tx.stealth_outputs_insert(utxo)
            }
        })?;
        Ok(())
    }

    pub fn update_utxo_status(
        &self,
        address: &UtxoAddress,
        is_burnt: Option<bool>,
        status: Option<OutputStatus>,
        is_frozen: Option<bool>,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.stealth_outputs_update(address, is_burnt, status, is_frozen))?;
        Ok(())
    }

    pub fn utxo_exists(&self, address: &UtxoAddress) -> Result<bool, StealthOutputsApiError> {
        let exists = self
            .store
            .with_read_tx(|tx| {
                // TODO(perf): consider a dedicated exists query
                tx.stealth_outputs_get_by_commitment(address.resource_address(), &address.id().into_commitment_bytes())
                    .optional()
            })?
            .is_some();
        Ok(exists)
    }

    pub fn utxos_get_many(
        &self,
        resource_address: &ResourceAddress,
        account: Option<&ComponentAddress>,
        by_status: Option<OutputStatus>,
    ) -> Result<Vec<StealthOutputModel>, StealthOutputsApiError> {
        let outputs = self
            .store
            .with_read_tx(|tx| tx.stealth_outputs_get_many(resource_address, account, by_status))?;
        Ok(outputs)
    }

    pub fn verify_and_update_outputs<'i, I: IntoIterator<Item = (UtxoAddress, &'i Utxo)>>(
        &self,
        outputs: I,
    ) -> Result<(), StealthOutputsApiError> {
        let all_used_view_only_keys = self
            .key_manager_api
            .get_all_keys(KeyBranch::ViewOnlyKey)?
            .into_iter()
            .map(|k| k.key_pair)
            .collect::<Vec<_>>();
        let network = self.config_api.get_network()?;

        let mut found_utxos_count = 0usize;
        let mut num_outputs = 0usize;
        for (addr, utxo) in outputs {
            num_outputs += 1;
            let commitment = addr.id().into_commitment_bytes();
            let resource_address = addr.resource_address();
            debug!(
                target: LOG_TARGET,
                "Validating UTXO for address: {}",
                addr,
            );

            let mut tx = self.store.create_write_tx()?;
            match tx
                .stealth_outputs_get_by_commitment(resource_address, &commitment)
                .optional()?
            {
                Some(_) => {
                    info!(
                        target: LOG_TARGET,
                        "Output already exists in the wallet. Updating. (commitment: {})",
                        commitment
                    );
                    if utxo.is_burnt() {
                        info!(
                            target: LOG_TARGET,
                            "🔥 Owned output is burnt with commitment: {}.",
                            commitment
                        );
                    }
                    if utxo.is_frozen() {
                        info!(
                            target: LOG_TARGET,
                            "❄️ Owned output is frozen with commitment: {}.",
                            commitment
                        );
                    }

                    // Update is_burnt and is_frozen status to false->true or true->false
                    tx.stealth_outputs_update(&addr, Some(utxo.is_burnt()), None, Some(utxo.is_frozen()))?;
                },
                None => {
                    let is_frozen = utxo.is_frozen();
                    let Some(output) = utxo.output() else {
                        debug!(
                            target: LOG_TARGET,
                            "Unknown Utxo output is burnt for commitment: {}. Skipping.",
                            commitment
                        );
                        continue;
                    };

                    // Output does not exist. Validate it and add it to the store
                    match self.validate_utxo(
                        &all_used_view_only_keys,
                        network,
                        *resource_address,
                        commitment,
                        output,
                        is_frozen,
                    ) {
                        Ok(Some(output)) => {
                            found_utxos_count += 1;
                            tx.stealth_outputs_insert(&output)?;
                        },
                        Ok(None) => {
                            debug!(
                                target: LOG_TARGET,
                                "🚮 wallet could not extract the value and mask for this output. Assuming it is not owned. (commitment: {})",
                                commitment
                            );
                        },
                        Err(e) => {
                            warn!(
                                target: LOG_TARGET,
                                "Output validation failed. Skipping. (commitment: {}, error: {})",
                                commitment,
                                e
                            );
                        },
                    }
                },
            }
            tx.commit()?;
        }

        if num_outputs > 0 {
            info!(
                target: LOG_TARGET,
                "✅️ Found {}/{} stealth outputs owned by this wallet.",
                found_utxos_count,
                num_outputs,
            );
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn validate_utxo(
        &self,
        all_used_account_view_only_keys: &[KeyPair],
        network: Network,
        resource_address: ResourceAddress,
        commitment: PedersenCommitmentBytes,
        output: &UtxoOutput,
        is_frozen: bool,
    ) -> Result<Option<StealthOutputModel>, StealthOutputsApiError> {
        // Validate the commitment is well-formed.
        let _output_commitment: PedersenCommitment =
            commitment
                .try_from_byte_type()
                .map_err(|e| StealthOutputsApiError::InvalidParameter {
                    param: "commitment",
                    reason: format!("Invalid output commitment bytes: {}", e),
                })?;

        let output_stealth_public_nonce =
            output
                .output
                .public_nonce
                .try_from_byte_type()
                .map_err(|e| StealthOutputsApiError::InvalidParameter {
                    param: "stealth_public_nonce",
                    reason: format!("Failed to parse stealth public nonce: {}", e),
                })?;

        debug!(
            target: LOG_TARGET,
            "Validating output using {} key(s) for resource address: {}, commitment: {}, public nonce: {}",
            all_used_account_view_only_keys.len(),
            resource_address,
            commitment,
            output_stealth_public_nonce,
        );

        for view_only_key in all_used_account_view_only_keys {
            trace!(
                target: LOG_TARGET,
                "Attempting to unblind output with view key index {} {}",
                view_only_key.key_index(),
                view_only_key.public_key
            );
            let unblinded_result = self.crypto_api.decrypt_value_and_mask(
                &output.output.encrypted_data,
                &commitment,
                &view_only_key.secret_key.key,
                &output_stealth_public_nonce,
            );

            let (value, owner_key, status) = match unblinded_result {
                Ok(mask_and_value) => {
                    let owner_key = self
                        .key_manager_api
                        .derive_account_key_pair(view_only_key.secret_key.key_index)?;
                    let stealth_secret = self.crypto_api.derive_stealth_owner_secret(
                        network,
                        &owner_key.secret_key.key,
                        &output_stealth_public_nonce,
                    );
                    let stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
                    if output.owner_public_key == stealth_address.to_byte_type() {
                        (mask_and_value.value, owner_key, OutputStatus::Unspent)
                    } else {
                        warn!(
                            target: LOG_TARGET,
                            "Output owner public key does not match the expected stealth address. (expected: {}, actual: {}). Utxo cannot be spent by this wallet and will be stored as invalid.",
                            stealth_address,
                            output.owner_public_key
                        );
                        (mask_and_value.value, owner_key, OutputStatus::Invalid)
                    }
                },
                Err(e) => {
                    debug!(
                        target: LOG_TARGET,
                        "Failed to unblind output for key {}. (commitment: {}, error: {})",
                        view_only_key.secret_key.key_index,
                        commitment,
                        e
                    );
                    continue;
                },
            };

            let owner_account = derive_component_address_from_public_key(
                &ACCOUNT_TEMPLATE_ADDRESS,
                &owner_key.public_key.to_byte_type(),
            );
            info!(
                target: LOG_TARGET,
                "🟢 Unblinded output for account {}. (commitment: {}, value: {})",
                owner_account,
                commitment,
                value,
            );

            return Ok(Some(StealthOutputModel {
                owner_account,
                // Note that this is not validated and depends on the caller ensuring the resource address belongs to
                // the stealth output.
                resource_address,
                commitment,
                value,
                sender_public_nonce: output_stealth_public_nonce.to_byte_type(),
                encryption_secret_key_index: view_only_key.key_index(),
                encrypted_data: output.output.encrypted_data.clone(),
                tag_byte: output.tag,
                status,
                is_burnt: false,
                is_frozen,
                is_on_chain: true,
                lock_id: None,
            }));
        }

        Ok(None)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StealthOutputsApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Crypto error: {0}")]
    Crypto(#[from] StealthCryptoApiError),
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Key manager error: {0}")]
    KeyManager(#[from] KeyManagerApiError),
    #[error("Accounts API error: {0}")]
    Accounts(#[from] AccountsApiError),
    #[error("Invalid parameter `{param}`: {reason}")]
    InvalidParameter { param: &'static str, reason: String },
    #[error("Config error: {0}")]
    ConfigApiError(#[from] ConfigApiError),
}

impl IsNotFoundError for StealthOutputsApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}
