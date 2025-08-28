//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::{debug, info, warn};
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
};
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
};
use tari_ootle_wallet_crypto::{kdfs, MaskAndValue, UnblindedStealthInputStatement};
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
        confidential_outputs::ConfidentialOutputsApiError,
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyBranch, KeyManagerApi, KeyManagerApiError},
        stealth_crypto::{StealthCryptoApi, StealthCryptoApiError},
    },
    models::{Account, KeyPair, OutputLockId, OutputStatus, StealthBalance, StealthOutputModel},
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

    pub fn lock_outputs_in_account_by_amount<A: Into<Amount>>(
        &self,
        account_address: &ComponentAddress,
        lock_id: OutputLockId,
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
            let (outputs, total_output_amount) = self.lock_outputs_internal(tx, account_address, amount, lock_id)?;

            if total_output_amount < amount {
                return Err(StealthOutputsApiError::InsufficientFunds);
            }

            Ok((outputs, total_output_amount))
        })
    }

    pub fn locks_set_transaction_id(
        &self,
        lock_id: OutputLockId,
        transaction_id: TransactionId,
    ) -> Result<(), ConfidentialOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.output_locks_set_params(lock_id, Some(transaction_id), None))?;
        Ok(())
    }

    pub fn lock_outputs_until_partial_amount(
        &self,
        account_address: &ComponentAddress,
        amount: Amount,
        locked_by_id: OutputLockId,
    ) -> Result<(Vec<StealthOutputModel>, Amount), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| self.lock_outputs_internal(tx, account_address, amount, locked_by_id))
    }

    fn lock_outputs_internal<TTx: WalletStoreWriter>(
        &self,
        tx: &mut TTx,
        account_address: &ComponentAddress,
        amount: Amount,
        locked_by_id: OutputLockId,
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
                .stealth_outputs_lock_smallest_amount(account_address, locked_by_id)
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

    pub fn create_lock_for_vault(&self, vault_id: &VaultId) -> Result<OutputLockId, StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        let lock_id = tx.output_locks_insert_for_vault(vault_id)?;
        tx.commit()?;
        Ok(lock_id)
    }

    pub fn create_lock_for_resource(
        &self,
        resource_address: &ResourceAddress,
    ) -> Result<OutputLockId, StealthOutputsApiError> {
        let lock_id = self
            .store
            .with_write_tx(|tx| tx.output_locks_insert(resource_address))?;
        Ok(lock_id)
    }

    pub fn release_locked_outputs(&self, lock_id: OutputLockId) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            tx.output_locks_delete(lock_id)?;
            tx.outputs_release_by_lock_id(lock_id)?;
            Ok(())
        })
    }

    pub fn finalize_outputs(&self, lock_id: OutputLockId) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            tx.output_locks_delete(lock_id)?;
            tx.stealth_outputs_finalize_by_lock_id(lock_id)?;
            Ok(())
        })
    }

    pub fn resolve_output_masks_for_spending(
        &self,
        owner_account: &Account,
        outputs: Vec<StealthOutputModel>,
    ) -> Result<Vec<UnblindedStealthInputStatement>, StealthOutputsApiError> {
        let network = self.config_api.get_network()?;
        // Derive owner secret - the sender does not know the owner secret
        let owner_key_part = self.key_manager_api.derive_account_key(owner_account.key_index())?;
        let mut outputs_with_masks = Vec::with_capacity(outputs.len());
        for output in outputs {
            // Derive the account secret, of which the public key is used by senders to encrypt the value and mask.
            let decryption_key_part = self
                .key_manager_api
                .derive_account_key(output.encryption_secret_key_index)?;
            // Derive the decryption key from the DHKE(sender's public nonce, encryption secret key);
            let nonce = RistrettoPublicKey::try_from_byte_type(&output.sender_public_nonce).map_err(|e| {
                StealthOutputsApiError::InvalidParameter {
                    param: "sender_public_nonce",
                    reason: format!("Sender public nonce bytes are not a canonical public key: {e}"),
                }
            })?;

            // Derive decryption shared secret
            let shared_decrypt_key = kdfs::encrypted_data_dh_kdf_aead(&decryption_key_part.key, &nonce);

            let (_, mask) = self.crypto_api.extract_value_and_mask(
                &shared_decrypt_key,
                &output.commitment,
                &output.encrypted_data,
            )?;

            let stealth_secret = self
                .crypto_api
                .derive_stealth_owner_secret(network, &owner_key_part.key, &nonce);

            outputs_with_masks.push(UnblindedStealthInputStatement {
                mask_and_value: MaskAndValue {
                    value: output.value,
                    mask,
                },
                owner_secret: stealth_secret,
                public_nonce: nonce,
            });
        }
        Ok(outputs_with_masks)
    }

    pub fn lock_revealed_funds(
        &self,
        lock_id: OutputLockId,
        amount_to_lock: Amount,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.vaults_lock_revealed_funds(lock_id, amount_to_lock))?;

        Ok(())
    }

    pub fn finalize_locked_revealed_funds(&self, lock_id: OutputLockId) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.vaults_finalized_locked_revealed_funds(lock_id)?;
        tx.commit()?;

        Ok(())
    }

    pub fn release_revealed_funds(&self, lock_id: OutputLockId) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.vaults_unlock_revealed_funds(lock_id)?;
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

    pub fn update_utxo_status_from_utxo(
        &self,
        address: &UtxoAddress,
        utxo: &Utxo,
    ) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            tx.stealth_outputs_update(address, Some(utxo.is_burnt()), None, Some(utxo.is_frozen()))
        })?;
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

    pub fn verify_and_update_outputs<'i, I: IntoIterator<Item = (UtxoAddress, &'i Utxo)>>(
        &self,
        outputs: I,
    ) -> Result<(), StealthOutputsApiError> {
        let all_used_account_keys = self.key_manager_api.get_all_keys(KeyBranch::Account)?;
        let all_used_account_keys = all_used_account_keys
            .into_iter()
            .map(|k| k.key_pair)
            .collect::<Vec<_>>();
        let network = self.config_api.get_network()?;

        let mut tx = self.store.create_write_tx()?;
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

                    // Output exists. We should never have the case this is marked as spent. TODO: Any other checks we
                    // need for this case?
                },
                None => {
                    if utxo.is_burnt() {
                        debug!(
                            target: LOG_TARGET,
                            "Unknown Utxo output is burnt for commitment: {}. Skipping.",
                            commitment
                        );
                        continue;
                    };

                    // Output does not exist. Validate it and add it to the store
                    match self.validate_utxo(&all_used_account_keys, network, *resource_address, commitment, utxo) {
                        Ok(Some(output)) => {
                            found_utxos_count += 1;
                            tx.stealth_outputs_insert(&output)?;
                        },
                        Ok(None) => {
                            debug!(
                                target: LOG_TARGET,
                                "❓️ wallet does not know how to extract the value and mask for this output. Assuming it is not owned. (commitment: {})",
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
        }
        tx.commit()?;

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

    pub fn validate_utxo(
        &self,
        all_used_account_keys: &[KeyPair],
        network: Network,
        resource_address: ResourceAddress,
        commitment: PedersenCommitmentBytes,
        utxo: &Utxo,
    ) -> Result<Option<StealthOutputModel>, StealthOutputsApiError> {
        // Validate the commitment is well-formed.
        let _output_commitment = PedersenCommitment::try_from_byte_type(&commitment).map_err(|e| {
            StealthOutputsApiError::InvalidParameter {
                param: "commitment",
                reason: format!("Invalid output commitment bytes: {}", e),
            }
        })?;
        let is_burnt = utxo.is_burnt();
        let is_frozen = utxo.is_frozen();
        let Some(output) = utxo.output() else {
            // We can't validate a burnt output
            return Ok(None);
        };

        let output_stealth_public_nonce =
            RistrettoPublicKey::try_from_byte_type(&output.output.public_nonce).map_err(|e| {
                StealthOutputsApiError::InvalidParameter {
                    param: "stealth_public_nonce",
                    reason: format!("Failed to parse stealth public nonce: {}", e),
                }
            })?;

        debug!(
            target: LOG_TARGET,
            "Validating output using {} key(s) for resource address: {}, commitment: {}, public nonce: {}",
            all_used_account_keys.len(),
            resource_address,
            commitment,
            output_stealth_public_nonce,
        );

        // TODO: limit accounts to those matching a tag
        for wallet_key in all_used_account_keys {
            let unblinded_result = self.crypto_api.unblind_output(
                &commitment,
                &output.output.encrypted_data,
                &wallet_key.secret_key.key,
                &output_stealth_public_nonce,
            );
            let (value, status) = match unblinded_result {
                Ok(mask_and_value) => {
                    let stealth_secret = self.crypto_api.derive_stealth_owner_secret(
                        network,
                        &wallet_key.secret_key.key,
                        &output_stealth_public_nonce,
                    );
                    let stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
                    if output.owner_public_key == stealth_address.to_byte_type() {
                        (mask_and_value.value, OutputStatus::Unspent)
                    } else {
                        warn!(
                            target: LOG_TARGET,
                            "Output owner public key does not match the expected stealth address. (expected: {}, actual: {}). Utxo cannot be spent by this wallet.",
                            stealth_address,
                            output.owner_public_key
                        );
                        (mask_and_value.value, OutputStatus::Invalid)
                    }
                },
                Err(e) => {
                    debug!(
                        target: LOG_TARGET,
                        "Failed to unblind output for key {}. (commitment: {}, error: {})",
                        wallet_key.secret_key.key_index,
                        commitment,
                        e
                    );
                    continue;
                },
            };

            let owner_account = derive_component_address_from_public_key(
                &ACCOUNT_TEMPLATE_ADDRESS,
                &wallet_key.public_key.to_byte_type(),
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
                encryption_secret_key_index: wallet_key.key_index(),
                encrypted_data: output.output.encrypted_data.clone(),
                tag_byte: output.tag,
                status,
                is_burnt,
                is_frozen,
                lock_id: None,
            }));
        }

        Ok(None)
    }

    pub fn set_transaction_hash_for_lock(
        &self,
        lock_id: OutputLockId,
        transaction_id: TransactionId,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.output_locks_set_params(lock_id, Some(transaction_id), None))?;
        Ok(())
    }

    pub fn set_vault_id_for_lock(
        &self,
        lock_id: OutputLockId,
        vault_id: VaultId,
    ) -> Result<(), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| tx.output_locks_set_params(lock_id, None, Some(vault_id)))?;
        Ok(())
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
