//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use ootle_byte_type::{ConvertFromByteType, ToByteType};
use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_engine_types::crypto::PrivateOutput;
use tari_ootle_common_types::optional::{IsNotFoundError, Optional};
use tari_ootle_wallet_crypto::{kdfs, MaskAndValue};
use tari_template_lib::types::{crypto::PedersenCommitmentBytes, Amount, VaultId};

use crate::{
    apis::{
        accounts::AccountsApiError,
        confidential_crypto::{ConfidentialCryptoApi, ConfidentialCryptoApiError},
        key_manager::{KeyManagerApi, KeyManagerApiError},
    },
    models::{Account, ConfidentialOutputModel, OutputStatus, WalletLockId, WalletSecretKey},
    storage::{
        CommittableStore,
        ReadableWalletStore,
        WalletStorageError,
        WalletStoreReader,
        WalletStoreWriter,
        WriteableWalletStore,
    },
    WalletSdkSpec,
};

const LOG_TARGET: &str = "tari::ootle::wallet_sdk::apis::confidential_outputs";

pub struct ConfidentialOutputsApi<'a, TSpec: WalletSdkSpec> {
    store: &'a TSpec::Store,
    key_manager_api: KeyManagerApi<'a, TSpec>,
    crypto_api: ConfidentialCryptoApi,
}

impl<'a, TSpec> ConfidentialOutputsApi<'a, TSpec>
where TSpec: WalletSdkSpec
{
    pub fn new(
        store: &'a TSpec::Store,
        key_manager_api: KeyManagerApi<'a, TSpec>,
        crypto_api: ConfidentialCryptoApi,
    ) -> Self {
        Self {
            store,
            key_manager_api,
            crypto_api,
        }
    }

    pub fn lock_outputs_by_amount<A: Into<Amount>>(
        &self,
        lock_id: WalletLockId,
        vault_id: &VaultId,
        amount: A,
    ) -> Result<(Vec<ConfidentialOutputModel>, Amount), ConfidentialOutputsApiError> {
        let amount =
            amount
                .into()
                .non_negative_checked()
                .ok_or_else(|| ConfidentialOutputsApiError::InvalidParameter {
                    param: "amount",
                    reason: "Amount must be non-negative".to_string(),
                })?;
        self.store.with_write_tx(|tx| {
            let (outputs, total_output_amount) = self.lock_outputs_internal(tx, lock_id, vault_id, amount)?;

            if total_output_amount < amount {
                return Err(ConfidentialOutputsApiError::InsufficientFunds);
            }

            Ok((outputs, total_output_amount))
        })
    }

    pub fn lock_outputs_until_partial_amount(
        &self,
        locked_id: WalletLockId,
        vault_id: &VaultId,
        amount: Amount,
    ) -> Result<(Vec<ConfidentialOutputModel>, Amount), ConfidentialOutputsApiError> {
        self.store
            .with_write_tx(|tx| self.lock_outputs_internal(tx, locked_id, vault_id, amount))
    }

    fn lock_outputs_internal<TTx: WalletStoreWriter>(
        &self,
        tx: &mut TTx,
        lock_id: WalletLockId,
        vault_id: &VaultId,
        amount: Amount,
    ) -> Result<(Vec<ConfidentialOutputModel>, Amount), ConfidentialOutputsApiError> {
        if amount.is_negative() {
            return Err(ConfidentialOutputsApiError::InvalidParameter {
                param: "amount",
                reason: "Amount cannot be negative".to_string(),
            });
        }
        let mut total_output_amount = Amount::zero();
        let mut outputs = Vec::new();
        while total_output_amount < amount {
            let output = tx
                .confidential_outputs_lock_smallest_amount(vault_id, lock_id)
                .optional()?;
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

    pub fn add_output(&self, output: ConfidentialOutputModel) -> Result<(), ConfidentialOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.confidential_outputs_insert(output)?;
        tx.commit()?;
        Ok(())
    }

    pub fn resolve_output_masks(
        &self,
        outputs: Vec<ConfidentialOutputModel>,
    ) -> Result<Vec<MaskAndValue>, ConfidentialOutputsApiError> {
        let mut outputs_with_masks = Vec::with_capacity(outputs.len());
        for output in outputs {
            // Encryption is always done with a DH of the account's public key
            let encryption_key = self.key_manager_api.get_key(output.view_only_key_id)?;
            // Either derive the mask from the sender's public nonce or from the local key manager
            let shared_decrypt_key = match output.sender_public_nonce {
                Some(nonce) => {
                    let nonce = RistrettoPublicKey::convert_from_byte_type(&nonce).map_err(|_| {
                        // We stored these outputs in the db, but they are malformed?
                        ConfidentialOutputsApiError::InvariantError {
                            details: format!(
                                "Invalid sender public nonce bytes ({}) for output commitment {}",
                                nonce, output.commitment
                            ),
                        }
                    })?;

                    // Derive shared secret
                    kdfs::encrypted_data_dh_kdf_aead(&encryption_key.secret, &nonce)
                },
                None => {
                    // Use local secret
                    encryption_key.secret
                },
            };

            let decrypted = self.crypto_api.decrypt_output_data(
                &shared_decrypt_key,
                &output.commitment,
                &output.encrypted_data,
                true,
            )?;

            // We're resolving for spending so we don't need the memo
            outputs_with_masks.push(decrypted.into_mask_and_value());
        }
        Ok(outputs_with_masks)
    }

    pub fn get_unspent_balance(&self, vault_id: &VaultId) -> Result<Amount, ConfidentialOutputsApiError> {
        let mut tx = self.store.create_read_tx()?;
        let balance = tx.confidential_outputs_get_unspent_balance(vault_id)?;
        Ok(balance.into())
    }

    pub fn verify_and_update_confidential_outputs<
        'i,
        I: IntoIterator<Item = (&'i PedersenCommitmentBytes, &'i PrivateOutput)>,
    >(
        &self,
        account: &Account,
        vault_id: VaultId,
        outputs: I,
    ) -> Result<(), ConfidentialOutputsApiError> {
        let view_key = self.key_manager_api.get_key(account.view_only_key_id)?;
        let mut tx = self.store.create_write_tx()?;

        for (commitment, output) in outputs {
            match tx
                .confidential_outputs_get_by_commitment(&vault_id, commitment)
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
                    // Output does not exist. Add it to the store
                    match self.validate_output(account, &view_key, vault_id, *commitment, output) {
                        Ok(output) => {
                            tx.confidential_outputs_insert(output)?;
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
        account: &Account,
        key: &WalletSecretKey,
        vault_id: VaultId,
        commitment: PedersenCommitmentBytes,
        output: &PrivateOutput,
    ) -> Result<ConfidentialOutputModel, ConfidentialOutputsApiError> {
        // Validate the commitment is well-formed.
        let _output_commitment = PedersenCommitment::convert_from_byte_type(&commitment).map_err(|e| {
            ConfidentialOutputsApiError::InvalidParameter {
                param: "commitment",
                reason: format!("Invalid output commitment bytes: {}", e),
            }
        })?;

        let output_stealth_public_nonce =
            RistrettoPublicKey::convert_from_byte_type(&output.public_nonce).map_err(|e| {
                ConfidentialOutputsApiError::InvalidParameter {
                    param: "stealth_public_nonce",
                    reason: format!("Failed to parse stealth public nonce: {}", e),
                }
            })?;

        let unblinded_result = self.crypto_api.unblind_output(
            &commitment,
            &output.encrypted_data,
            &key.secret,
            &output_stealth_public_nonce,
            false,
        );
        let (value, memo, status) = match unblinded_result {
            Ok(output) => (output.value(), output.memo, OutputStatus::Unspent),
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "Failed to unblind output. (commitment: {}, error: {})",
                    commitment,
                    e
                );
                (0, None, OutputStatus::Invalid)
            },
        };

        Ok(ConfidentialOutputModel {
            account_address: account.component_address,
            vault_id,
            commitment,
            value: value.into(),
            sender_public_nonce: Some(output_stealth_public_nonce.to_byte_type()),
            view_only_key_id: key.key_id,
            owner_key_id: account.owner_key_id,
            encrypted_data: output.encrypted_data.clone(),
            public_asset_tag: None,
            memo,
            status,
            lock_id: None,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfidentialOutputsApiError {
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
    #[error("BUG: Invariant error: {details}")]
    InvariantError { details: String },
}

impl IsNotFoundError for ConfidentialOutputsApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}
