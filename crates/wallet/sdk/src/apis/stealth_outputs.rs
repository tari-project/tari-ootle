//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::{info, warn};
use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_engine_types::{FromByteType, ToByteType, UtxoOutput};
use tari_key_manager::key_manager::DerivedKey;
use tari_ootle_common_types::optional::{IsNotFoundError, Optional};
use tari_ootle_wallet_crypto::{kdfs, MaskAndValue, UnblindedStealthInputStatement};
use tari_template_lib::{
    models::{ResourceAddress, VaultId},
    prelude::{ComponentAddress, PedersenCommitmentBytes},
    types::Amount,
};
use tari_transaction::TransactionId;

use crate::{
    apis::{
        accounts::AccountsApiError,
        confidential_crypto::{ConfidentialCryptoApi, ConfidentialCryptoApiError},
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyManagerApi, KeyManagerApiError},
    },
    models::{AccountWithPublicKey, OutputLockId, OutputStatus, StealthOutputModel},
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

const LOG_TARGET: &str = "tari::wallet::apis::stealth_outputs";

pub struct StealthOutputsApi<'a, TStore> {
    store: &'a TStore,
    key_manager_api: KeyManagerApi<'a, TStore>,
    crypto_api: ConfidentialCryptoApi,
    config_api: ConfigApi<'a, TStore>,
}

impl<'a, TStore: WalletStore> StealthOutputsApi<'a, TStore> {
    pub fn new(
        store: &'a TStore,
        key_manager_api: KeyManagerApi<'a, TStore>,
        crypto_api: ConfidentialCryptoApi,
        config_api: ConfigApi<'a, TStore>,
    ) -> Self {
        Self {
            store,
            key_manager_api,
            crypto_api,
            config_api,
        }
    }

    pub fn lock_outputs_by_amount<A: Into<Amount>>(
        &self,
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
            let (outputs, total_output_amount) = self.lock_outputs_internal(tx, amount, lock_id)?;

            if total_output_amount < amount {
                return Err(StealthOutputsApiError::InsufficientFunds);
            }

            Ok((outputs, total_output_amount))
        })
    }

    pub fn lock_outputs_until_partial_amount(
        &self,
        amount: Amount,
        locked_by_id: OutputLockId,
    ) -> Result<(Vec<StealthOutputModel>, Amount), StealthOutputsApiError> {
        self.store
            .with_write_tx(|tx| self.lock_outputs_internal(tx, amount, locked_by_id))
    }

    fn lock_outputs_internal<TTx: WalletStoreWriter>(
        &self,
        tx: &mut TTx,
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
            let output = tx.stealth_outputs_lock_smallest_amount(locked_by_id).optional()?;
            match output {
                Some(output) => {
                    total_output_amount += output.value;
                    outputs.push(output);
                },
                None => {
                    break;
                },
            }
        }

        Ok((outputs, total_output_amount))
    }

    pub fn add_output(&self, output: StealthOutputModel) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.stealth_outputs_insert(output)?;
        tx.commit()?;
        Ok(())
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

    pub fn release_proof_outputs(&self, lock_id: OutputLockId) -> Result<(), StealthOutputsApiError> {
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
        owner_account: &AccountWithPublicKey,
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

            let stealth_secret = kdfs::owner_stealth_dh_secret(network, &owner_key_part.key, &nonce);

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

    pub fn get_unspent_balance(&self, vault_id: &VaultId) -> Result<Amount, StealthOutputsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let balance = tx.outputs_get_unspent_balance(vault_id)?;
        Ok(balance.into())
    }

    pub fn verify_and_update_outputs<'i, I: IntoIterator<Item = (&'i PedersenCommitmentBytes, &'i UtxoOutput)>>(
        &self,
        account: &AccountWithPublicKey,
        resource_address: ResourceAddress,
        outputs: I,
    ) -> Result<(), StealthOutputsApiError> {
        let key = self.key_manager_api.derive_account_key(account.key_index())?;
        let mut tx = self.store.create_write_tx()?;

        for (commitment, output) in outputs {
            match tx
                .stealth_outputs_get_by_commitment(&resource_address, commitment)
                .optional()?
            {
                Some(_) => {
                    info!(
                        target: LOG_TARGET,
                        "Output already exists in the wallet. Skipping. (commitment: {})",
                        commitment
                    );
                    // Output exists. We should never have the case this is marked as spent. Should we check that?
                },
                None => {
                    // Output does not exist. Validate it and add it to the store
                    match self.validate_output(*account.address(), resource_address, &key, *commitment, output) {
                        Ok(output) => {
                            tx.stealth_outputs_insert(output)?;
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

        Ok(())
    }

    fn validate_output(
        &self,
        account_address: ComponentAddress,
        resource_address: ResourceAddress,
        key: &DerivedKey<RistrettoPublicKey>,
        commitment: PedersenCommitmentBytes,
        utxo: &UtxoOutput,
    ) -> Result<StealthOutputModel, StealthOutputsApiError> {
        // Validate the commitment is well-formed.
        let _output_commitment = PedersenCommitment::try_from_byte_type(&commitment).map_err(|e| {
            StealthOutputsApiError::InvalidParameter {
                param: "commitment",
                reason: format!("Invalid output commitment bytes: {}", e),
            }
        })?;

        let output_stealth_public_nonce =
            RistrettoPublicKey::try_from_byte_type(&utxo.output.public_nonce).map_err(|e| {
                StealthOutputsApiError::InvalidParameter {
                    param: "stealth_public_nonce",
                    reason: format!("Failed to parse stealth public nonce: {}", e),
                }
            })?;

        let unblinded_result = self.crypto_api.unblind_output(
            &commitment,
            &utxo.output.encrypted_data,
            &key.key,
            &output_stealth_public_nonce,
        );
        let (value, status) = match unblinded_result {
            Ok(output) => (output.value, OutputStatus::Unspent),
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Failed to unblind output. (commitment: {}, error: {})",
                    commitment,
                    e
                );
                (Amount::zero(), OutputStatus::Invalid)
            },
        };

        Ok(StealthOutputModel {
            owner_account: account_address,
            // Note that this is not validated and depends on the caller ensuring the resource address belongs to the
            // stealth output.
            resource_address,
            commitment,
            value,
            sender_public_nonce: output_stealth_public_nonce.to_byte_type(),
            encryption_secret_key_index: key.key_index,
            encrypted_data: utxo.output.encrypted_data.clone(),
            status,
            lock_id: None,
        })
    }

    pub fn set_transaction_hash_for_lock(
        &self,
        lock_id: OutputLockId,
        transaction_id: TransactionId,
    ) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.output_locks_set_transaction_id(lock_id, transaction_id)?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StealthOutputsApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Confidential crypto error: {0}")]
    ConfidentialCrypto(#[from] ConfidentialCryptoApiError),
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
