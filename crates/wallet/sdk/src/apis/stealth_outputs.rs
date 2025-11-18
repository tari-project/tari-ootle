//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use digest::crypto_common::rand_core::OsRng;
use log::*;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey},
};
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    limits,
    FromByteType,
    ToByteType,
    Utxo,
    UtxoOutput,
};
use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
};
use tari_ootle_wallet_crypto::{
    memo::Memo,
    DecryptedData,
    UnblindedOutputWitness,
    UnblindedStealthInputWitness,
    UnblindedStealthOutputWitness,
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, StealthTransferStatement, UtxoAddress},
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    types::{Amount, EncryptedData},
};

use crate::{
    apis::{
        accounts::AccountsApiError,
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyManagerApi, KeyManagerApiError},
        stealth_crypto::{StealthCryptoApi, StealthCryptoApiError},
        stealth_transfer::{StealthOutputToCreate, UnblindedInputToSpend},
    },
    models::{
        input_selection,
        input_selection::{branch_and_bound::KeyedInput, InputSelectionAlgorithm},
        AccountAndViewKeys,
        InputSpendData,
        KeyBranch,
        KeyId,
        OutputStatus,
        StealthBalance,
        StealthOutputInfo,
        StealthOutputModel,
        WalletLockId,
    },
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

const LOG_TARGET: &str = "tari::ootle::wallet::apis::stealth_outputs";

pub struct StealthOutputsApi<'a, TSpec: WalletSdkSpec> {
    store: &'a TSpec::Store,
    key_manager_api: KeyManagerApi<'a, TSpec>,
    crypto_api: StealthCryptoApi,
    config_api: ConfigApi<'a, TSpec::Store>,
}

impl<'a, TSpec: WalletSdkSpec> StealthOutputsApi<'a, TSpec> {
    pub fn new(
        store: &'a TSpec::Store,
        key_manager_api: KeyManagerApi<'a, TSpec>,
        crypto_api: StealthCryptoApi,
        config_api: ConfigApi<'a, TSpec::Store>,
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
    ) -> Result<(Vec<InputSpendData>, Amount), StealthOutputsApiError> {
        let amount = amount
            .into()
            .non_negative_checked()
            .ok_or_else(|| StealthOutputsApiError::InvalidParameter {
                param: "amount",
                reason: "lock_outputs_for_at_least_amount: Amount must be non-negative".to_string(),
            })?;
        if amount.is_zero() {
            return Ok((Vec::new(), Amount::zero()));
        }

        self.store.with_write_tx(|tx| {
            let (outputs, total_output_amount) = self.lock_outputs_internal(
                tx,
                account_address,
                resource_address,
                amount,
                lock_id,
                InputSelectionAlgorithm::BranchAndBound,
            )?;

            if total_output_amount < amount {
                return Err(StealthOutputsApiError::InsufficientFunds);
            }
            Ok((outputs, total_output_amount))
        })
    }

    /// Locks as many outputs required to reach at least the specified amount. If there are insufficient funds, all
    /// available outputs will be locked and returned along with the total amount locked.
    pub fn lock_outputs_until_partial_amount(
        &self,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        amount: Amount,
        locked_by_id: WalletLockId,
    ) -> Result<(Vec<InputSpendData>, Amount), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            self.lock_outputs_internal(
                tx,
                account_address,
                resource_address,
                amount,
                locked_by_id,
                InputSelectionAlgorithm::SmallestFirst,
            )
        })
    }

    fn lock_outputs_internal(
        &self,
        tx: &mut <TSpec::Store as WriteableWalletStore>::WriteTransaction<'_>,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        amount: Amount,
        locked_by_id: WalletLockId,
        selection_algo: InputSelectionAlgorithm,
    ) -> Result<(Vec<InputSpendData>, Amount), StealthOutputsApiError> {
        if amount.is_negative() {
            return Err(StealthOutputsApiError::InvalidParameter {
                param: "amount",
                reason: "lock_outputs_internal: Amount cannot be negative".to_string(),
            });
        }

        const INPUT_LIMIT: usize = limits::STEALTH_LIMITS.max_inputs;

        match selection_algo {
            InputSelectionAlgorithm::SmallestFirst => {
                let mut total_output_amount = Amount::zero();
                let mut outputs = Vec::new();
                while total_output_amount < amount {
                    if outputs.len() >= INPUT_LIMIT {
                        warn!(
                            target: LOG_TARGET,
                            "Reached maximum input limit of {} when locking outputs.",
                            INPUT_LIMIT
                        );
                        break;
                    }
                    let output = tx
                        .stealth_outputs_lock_smallest_amount(account_address, resource_address, locked_by_id)
                        .optional()?;
                    match output {
                        Some(output) => {
                            total_output_amount += Amount::from(output.value);
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

                let outputs = outputs.into_iter().map(|i| i.into_spend_data()).collect();
                Ok((outputs, total_output_amount))
            },
            InputSelectionAlgorithm::BranchAndBound => {
                let unspent =
                    tx.stealth_outputs_get_unspent_for_spending(account_address, resource_address, locked_by_id)?;

                let mut unspent = unspent
                    .into_iter()
                    .map(|o| (o.commitment, InputSpendData::from(o)))
                    .collect::<HashMap<_, _>>();

                let inputs = unspent
                    .values()
                    .map(|o| KeyedInput::new(o.commitment, o.value))
                    .collect::<Vec<_>>();

                // TODO: note that the behaviour of this implementation does not allow for partial selection, needed by
                // UtxoInputSelection::PreferConfidential. For now, we prevent running into this by only
                // using SmallestFirst.
                let result = input_selection::branch_and_bound::select(&inputs, amount, INPUT_LIMIT).ok_or(
                    StealthOutputsApiError::InputSelectionFailed {
                        details: "Failed to select inputs using branch and bound algorithm".to_string(),
                    },
                )?;

                let outputs = result
                    .selected_keys()
                    .iter()
                    .take(INPUT_LIMIT)
                    .map(|selected| {
                        unspent
                            .remove(*selected)
                            .expect("selected an output not in the input key set")
                    })
                    .collect::<Vec<_>>();

                // Lock the selected outputs
                tx.stealth_outputs_lock_many(resource_address, result.selected_keys(), locked_by_id)?;

                Ok((outputs, result.total_value()))
            },
        }
    }

    pub fn add_output(&self, output: &StealthOutputModel) -> Result<(), StealthOutputsApiError> {
        let mut tx = self.store.create_write_tx()?;
        tx.stealth_outputs_insert(output)?;
        tx.commit()?;
        Ok(())
    }

    fn resolve_output_masks_for_spending(
        &self,
        owner_key_id: KeyId,
        view_only_key_id: KeyId,
        inputs: &[InputSpendData],
    ) -> Result<Vec<UnblindedInputToSpend>, StealthOutputsApiError> {
        let network = self.config_api.get_network()?;

        let owner_key_part = self.key_manager_api.get_key(owner_key_id)?;
        let mut inputs_with_masks = Vec::with_capacity(inputs.len());
        for input in inputs {
            // Derive the decryption key from the DHKE(sender's public nonce, encryption secret key);
            let nonce =
                input
                    .public_nonce
                    .try_from_byte_type()
                    .map_err(|e| StealthOutputsApiError::InvalidParameter {
                        param: "sender_public_nonce",
                        reason: format!("Sender public nonce bytes are not a canonical public key: {e}"),
                    })?;

            // Derive the view-only secret, of which the public key is used by senders to encrypt the value and mask.
            let decrypted = self.decrypt_value_and_mask(
                &input.encrypted_data,
                &input.commitment,
                view_only_key_id,
                &nonce,
                // We don't need to decrypt the memo to spend the output
                true,
            )?;

            let stealth_secret = self
                .crypto_api
                .derive_stealth_owner_secret(network, &owner_key_part.secret, &nonce);

            inputs_with_masks.push(UnblindedInputToSpend {
                witness: UnblindedStealthInputWitness {
                    mask_and_value: decrypted.mask_and_value,
                    owner_secret: stealth_secret,
                    public_nonce: nonce,
                },
            });
        }
        Ok(inputs_with_masks)
    }

    pub fn get_unspent_outputs_by_account(
        &self,
        account_address: &ComponentAddress,
        exclude_locked: bool,
    ) -> Result<Vec<StealthOutputInfo>, StealthOutputsApiError> {
        let balance = self
            .store
            .with_read_tx(|tx| tx.stealth_outputs_get_unspent_by_account(account_address, None, exclude_locked))?;
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

    pub fn count_unspent_outputs_for_account(
        &self,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
    ) -> Result<u64, StealthOutputsApiError> {
        let count = self.store.with_read_tx(|tx| {
            tx.stealth_outputs_count_by_status(account_address, resource_address, OutputStatus::Unspent)
        })?;
        Ok(count)
    }

    pub fn upsert_utxo(&self, utxo: &StealthOutputModel) -> Result<(), StealthOutputsApiError> {
        self.store.with_write_tx(|tx| {
            // TODO(perf): consider a dedicated exists query
            let maybe_utxo = tx
                .stealth_outputs_get_by_commitment(&utxo.resource_address, &utxo.commitment)
                .optional()?;
            if let Some(prev_utxo) = maybe_utxo {
                let new_status = match prev_utxo.status {
                    OutputStatus::Unspent => Some(utxo.status),
                    // If not unspent, don't allow status to be changed.
                    // EDGE-CASE: scanning picks up a local UTXO that we know was spent
                    _ => None,
                };
                let address = utxo.to_utxo_address();
                tx.stealth_outputs_update(&address, Some(utxo.is_burnt), new_status, Some(utxo.is_frozen))
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

    #[allow(clippy::too_many_lines)]
    pub fn verify_and_update_outputs<'i, I: IntoIterator<Item = (UtxoAddress, &'i Utxo)>>(
        &self,
        outputs: I,
    ) -> Result<(), StealthOutputsApiError> {
        let all_used_view_only_keys = self
            .key_manager_api
            .get_all_derived_keys(KeyBranch::ViewOnlyKey)?
            .into_iter()
            .map(|view_key| {
                let account_key = self.key_manager_api.derive_account_key(
                    view_key
                        .key_id
                        .derived_index()
                        .expect("get_all_derived_keys returns only derived keys"),
                )?;
                Ok::<_, KeyManagerApiError>(AccountAndViewKeys {
                    account_public_key: account_key.to_public_key().to_byte_type(),
                    account_key: Some(account_key.into()),
                    view_only_key: view_key.into(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
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
        all_used_account_view_only_keys: &[AccountAndViewKeys],
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

        for keys in all_used_account_view_only_keys {
            trace!(
                target: LOG_TARGET,
                "Attempting to unblind output with view key {}",
                keys.view_only_key.key_id,
            );
            let unblinded_result = self.crypto_api.decrypt_value_and_mask(
                &output.output.encrypted_data,
                &commitment,
                &keys.view_only_key.secret,
                &output_stealth_public_nonce,
                false,
            );

            let (value, memo, status) = match unblinded_result {
                Ok(decrypted) => {
                    if let Some(ref owner_key) = keys.account_key {
                        let stealth_secret = self.crypto_api.derive_stealth_owner_secret(
                            network,
                            &owner_key.secret,
                            &output_stealth_public_nonce,
                        );
                        let stealth_address = RistrettoPublicKey::from_secret_key(&stealth_secret);
                        if output.owner_public_key == stealth_address.to_byte_type() {
                            (decrypted.value(), decrypted.memo, OutputStatus::Unspent)
                        } else {
                            warn!(
                                target: LOG_TARGET,
                                "⚠️ Output owner public key does not match the expected stealth address. (expected: {}, actual: {}). Utxo cannot be spent by this wallet and will be stored as invalid.",
                                stealth_address,
                                output.owner_public_key
                            );
                            (decrypted.value(), decrypted.memo, OutputStatus::Invalid)
                        }
                    } else {
                        info!(
                            target: LOG_TARGET,
                            "Output can only be viewed, not spent, as there is no owner key for view key {}",
                            keys.view_only_key.key_id,
                        );
                        (decrypted.value(), decrypted.memo, OutputStatus::Unspent)
                    }
                },
                Err(e) => {
                    debug!(
                        target: LOG_TARGET,
                        "Failed to unblind output for key {}. (commitment: {}, error: {})",
                        keys.view_only_key.key_id,
                        commitment,
                        e
                    );
                    continue;
                },
            };

            let owner_account =
                derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &keys.account_public_key);
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
                view_only_key_id: keys.view_only_key.key_id,
                owner_key_id: keys.account_key.as_ref().map(|k| k.key_id),
                encrypted_data: output.output.encrypted_data.clone(),
                tag_byte: output.tag,
                memo,
                minimum_value_promise: output.output.minimum_value_promise,
                status,
                is_burnt: false,
                is_frozen,
                is_on_chain: true,
                lock_id: None,
            }));
        }

        Ok(None)
    }

    pub fn create_output_witness(
        &self,
        destination: &RistrettoOotleAddress,
        amount: u64,
        resource_address: &ResourceAddress,
        resource_view_key: Option<RistrettoPublicKey>,
        memo: Option<&Memo>,
    ) -> Result<UnblindedStealthOutputWitness, StealthOutputsApiError> {
        let mask = self.key_manager_api.next_key(KeyBranch::StealthMask)?;

        let (nonce_secret, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);
        let encrypted_data = self.crypto_api.encrypt_value_and_mask(
            amount,
            &mask.key,
            destination.view_only_key(),
            &nonce_secret,
            memo,
        )?;

        // Create stealth address - used during spend time
        let output_owner_public_key = self.crypto_api.derive_stealth_owner_public_key(
            destination.network(),
            destination.account_key(),
            &nonce_secret,
        );

        let witness = UnblindedOutputWitness {
            amount,
            mask: mask.key,
            sender_public_nonce: public_nonce,
            encrypted_data,
            minimum_value_promise: 0,
            resource_view_key,
        };

        let derived_tag = self.crypto_api.derive_stealth_output_tag(
            destination.network(),
            &nonce_secret,
            destination.view_only_key(),
            resource_address,
        );

        Ok(UnblindedStealthOutputWitness {
            witness,
            output_owner_public_key,
            tag: derived_tag,
        })
    }

    pub fn generate_transfer_statement<I>(
        &self,
        params: TransferStatementParams<'_, I>,
    ) -> Result<StealthTransferStatement, StealthOutputsApiError>
    where
        I: IntoIterator<Item = StealthOutputToCreate<'a>>,
    {
        let unblinded_inputs =
            self.resolve_output_masks_for_spending(params.spend_key_id, params.view_only_key_id, params.inputs)?;
        let outputs = params
            .outputs
            .into_iter()
            .map(|output| {
                self.create_output_witness(
                    &output.owner_address,
                    output.amount,
                    params.resource_address,
                    params.resource_view_key.clone(),
                    output.memo,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let total_input_amount =
            unblinded_inputs.iter().map(|i| Amount::from(i.value())).sum::<Amount>() + params.input_revealed_amount;
        let total_output_amount =
            outputs.iter().map(|o| Amount::from(o.witness.amount)).sum::<Amount>() + params.output_revealed_amount;
        if total_input_amount != total_output_amount {
            return Err(StealthOutputsApiError::InvalidParameter {
                param: "inputs/outputs",
                reason: format!(
                    "Input and output amounts do not balance. Input: {}, Output: {}",
                    total_input_amount, total_output_amount
                ),
            });
        }

        let statement = self.crypto_api.generate_transfer_statement(
            unblinded_inputs.iter().map(|i| &i.witness),
            params.input_revealed_amount,
            outputs.iter(),
            params.output_revealed_amount,
            params.required_signer,
        )?;
        Ok(statement)
    }

    pub fn decrypt_value_and_mask(
        &self,
        output_encrypted_value: &EncryptedData,
        output_commitment: &PedersenCommitmentBytes,
        claim_secret_key_id: KeyId,
        reciprocal_public_key: &RistrettoPublicKey,
        skip_memo: bool,
    ) -> Result<DecryptedData, StealthOutputsApiError> {
        let key = self.key_manager_api.get_key(claim_secret_key_id)?;
        let decrypted = self.crypto_api.decrypt_value_and_mask(
            output_encrypted_value,
            output_commitment,
            &key.secret,
            reciprocal_public_key,
            skip_memo,
        )?;
        Ok(decrypted)
    }

    pub fn encrypt_value_and_mask(
        &self,
        amount: u64,
        public_key: &RistrettoPublicKey,
        memo: Option<&Memo>,
    ) -> Result<(RistrettoPublicKey, EncryptedData), StealthOutputsApiError> {
        let nonce_secret = self.key_manager_api.create_throwaway_nonce();
        let public_nonce = RistrettoPublicKey::from_secret_key(&nonce_secret);
        let mask = self.key_manager_api.next_key(KeyBranch::StealthMask)?;
        let data = self
            .crypto_api
            .encrypt_value_and_mask(amount, &mask.key, public_key, &nonce_secret, memo)?;
        Ok((public_nonce, data))
    }
}

pub struct TransferStatementParams<'a, I> {
    pub spend_key_id: KeyId,
    pub view_only_key_id: KeyId,
    pub resource_address: &'a ResourceAddress,
    pub resource_view_key: Option<RistrettoPublicKey>,
    pub inputs: &'a [InputSpendData],
    pub input_revealed_amount: Amount,
    pub outputs: I,
    pub output_revealed_amount: Amount,
    pub required_signer: RistrettoPublicKeyBytes,
}

#[derive(Debug, thiserror::Error)]
pub enum StealthOutputsApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Crypto error: {0}")]
    Crypto(#[from] StealthCryptoApiError),
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Input selection error: {details}")]
    InputSelectionFailed { details: String },
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
