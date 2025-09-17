//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};

use tari_engine_types::{resource::Resource, substate::SubstateId, UtxoAddress, UtxoId};
use tari_ootle_common_types::{
    optional::IsNotFoundError,
    shard::Shard,
    substate_type::SubstateType,
    StateVersion,
    VersionedSubstateIdRef,
};
use tari_template_lib::{
    models::VaultId,
    prelude::{
        ComponentAddress,
        NonFungibleId,
        PedersenCommitmentBytes,
        ResourceAddress,
        ResourceType,
        RistrettoPublicKeyBytes,
    },
    types::{crypto::UtxoTag, Amount, TemplateAddress},
};
use tari_transaction::{Transaction, TransactionId};
use webauthn_rs::prelude::Passkey;

use crate::models::{
    Account,
    AccountUpdate,
    AuthoredTemplateModel,
    ConfidentialOutputModel,
    Config,
    NewAccountData,
    NonFungibleToken,
    OutputStatus,
    ResourceModel,
    StealthBalance,
    StealthOutputModel,
    SubstateModel,
    TransactionStatus,
    UtxoUnspent,
    VaultModel,
    WalletLockId,
    WalletTransaction,
    WalletTransactionUpdate,
};

pub trait WalletStore {
    type ReadTransaction<'a>: WalletStoreReader
    where Self: 'a;
    type WriteTransaction<'a>: WalletStoreWriter + Deref<Target = Self::ReadTransaction<'a>> + DerefMut
    where Self: 'a;

    fn create_read_tx(&self) -> Result<Self::ReadTransaction<'_>, WalletStorageError>;
    fn create_write_tx(&self) -> Result<Self::WriteTransaction<'_>, WalletStorageError>;

    fn with_write_tx<F: FnOnce(&mut Self::WriteTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<WalletStorageError> {
        let mut tx = self.create_write_tx()?;
        match f(&mut tx) {
            Ok(r) => {
                tx.commit()?;
                Ok(r)
            },
            Err(e) => {
                if let Err(err) = tx.rollback() {
                    log::error!("Failed to rollback transaction: {}", err);
                }
                Err(e)
            },
        }
    }

    fn with_read_tx<F: FnOnce(&mut Self::ReadTransaction<'_>) -> Result<R, E>, R, E>(&self, f: F) -> Result<R, E>
    where E: From<WalletStorageError> {
        let mut tx = self.create_read_tx()?;
        let ret = f(&mut tx)?;
        Ok(ret)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletStorageError {
    #[error("General database failure for operation {operation}: {details}")]
    GeneralFailure { operation: &'static str, details: String },
    #[error("Bad query for operation {operation}: {details}")]
    BadQuery { operation: &'static str, details: String },
    #[error("Failed to decode for operation {operation} on {item}: {details}")]
    DecodingError {
        operation: &'static str,
        item: &'static str,
        details: String,
    },
    #[error("Failed to encode for operation {operation} on {item}: {details}")]
    EncodingError {
        operation: &'static str,
        item: &'static str,
        details: String,
    },
    #[error("[{operation}] {entity} not found with key {key}")]
    NotFound {
        operation: &'static str,
        entity: String,
        key: String,
    },
    #[error("Operation error {operation}: {details}")]
    OperationError { operation: &'static str, details: String },
    #[error("Data inconsistency for operation {operation}: {details}")]
    DataInconsistent { operation: &'static str, details: String },
}

impl IsNotFoundError for WalletStorageError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

impl WalletStorageError {
    pub fn general<E: std::fmt::Display>(operation: &'static str, e: E) -> Self {
        Self::GeneralFailure {
            operation,
            details: e.to_string(),
        }
    }

    pub fn bad_query<E: Into<String>>(operation: &'static str, details: E) -> Self {
        Self::BadQuery {
            operation,
            details: details.into(),
        }
    }

    pub fn not_found(operation: &'static str, entity: String, key: String) -> Self {
        Self::NotFound { operation, entity, key }
    }
}

pub trait WalletStoreReader {
    // Key manager
    fn key_manager_get_all(&mut self, branch: &str) -> Result<Vec<(u64, bool)>, WalletStorageError>;
    fn key_manager_get_active_index(&mut self, branch: &str) -> Result<u64, WalletStorageError>;
    fn key_manager_get_last_index(&mut self, branch: &str) -> Result<u64, WalletStorageError>;
    // Config
    fn config_get<T: serde::de::DeserializeOwned>(&mut self, key: &str) -> Result<Config<T>, WalletStorageError>;
    fn config_get_string(&mut self, key: &str) -> Result<Config<String>, WalletStorageError>;
    fn config_exists(&mut self, key: &str) -> Result<bool, WalletStorageError>;
    // JWT
    fn jwt_get_all(&mut self) -> Result<Vec<(i32, Option<String>)>, WalletStorageError>;
    // Transactions
    fn transactions_get(&mut self, transaction_id: TransactionId) -> Result<WalletTransaction, WalletStorageError>;
    fn transactions_fetch_all(
        &mut self,
        status: Option<TransactionStatus>,
        component: Option<ComponentAddress>,
        signed_by_public_key: Option<RistrettoPublicKeyBytes>,
    ) -> Result<Vec<WalletTransaction>, WalletStorageError>;
    // Substates
    fn substates_get(&mut self, address: &SubstateId) -> Result<SubstateModel, WalletStorageError>;
    fn substates_get_all(
        &mut self,
        by_type: Option<SubstateType>,
        by_template_address: Option<&TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<SubstateModel>, WalletStorageError>;
    fn substates_get_children(&mut self, parent: &SubstateId) -> Result<Vec<SubstateModel>, WalletStorageError>;
    // Accounts
    fn accounts_get(&mut self, address: &ComponentAddress) -> Result<Account, WalletStorageError>;
    fn accounts_get_many(&mut self, offset: u64, limit: u64) -> Result<Vec<Account>, WalletStorageError>;
    fn accounts_get_default(&mut self) -> Result<Account, WalletStorageError>;
    fn accounts_count(&mut self) -> Result<u64, WalletStorageError>;
    fn accounts_get_by_name(&mut self, name: &str) -> Result<Account, WalletStorageError>;
    fn accounts_get_by_vault(&mut self, vault_id: &VaultId) -> Result<Account, WalletStorageError>;
    fn accounts_get_associated_stealth_resources(
        &mut self,
        address: &ComponentAddress,
    ) -> Result<HashSet<ResourceAddress>, WalletStorageError>;

    // Vaults
    fn vaults_get(&mut self, vault_id: &VaultId) -> Result<VaultModel, WalletStorageError>;
    fn vaults_exists(&mut self, vault_id: &VaultId) -> Result<bool, WalletStorageError>;
    fn vaults_get_by_resource(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
    ) -> Result<VaultModel, WalletStorageError>;
    fn vaults_get_by_account(&mut self, account_addr: &ComponentAddress)
        -> Result<Vec<VaultModel>, WalletStorageError>;

    // Resources
    fn resources_get(&mut self, resource_address: &ResourceAddress) -> Result<ResourceModel, WalletStorageError>;
    fn resources_get_by_type(&mut self, resource_type: ResourceType) -> Result<Vec<ResourceModel>, WalletStorageError>;
    fn resources_get_many<'a, I: IntoIterator<Item = &'a ResourceAddress>>(
        &mut self,
        addresses: I,
    ) -> Result<Vec<ResourceModel>, WalletStorageError>;

    // Outputs
    fn outputs_get_unspent_balance(&mut self, vault_id: &VaultId) -> Result<u64, WalletStorageError>;
    fn outputs_get_locked_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
    ) -> Result<Vec<ConfidentialOutputModel>, WalletStorageError>;
    fn outputs_get_by_commitment(
        &mut self,
        vault_id: &VaultId,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<ConfidentialOutputModel, WalletStorageError>;

    fn outputs_get_by_account_and_status(
        &mut self,
        account_addr: &ComponentAddress,
        status: OutputStatus,
    ) -> Result<Vec<ConfidentialOutputModel>, WalletStorageError>;

    // Stealth outputs
    fn stealth_outputs_get_unspent_balance(
        &mut self,
        resource_address: &ResourceAddress,
    ) -> Result<StealthBalance, WalletStorageError>;

    fn stealth_outputs_get_unspent_by_account(
        &mut self,
        account_addr: &ComponentAddress,
    ) -> Result<Vec<StealthOutputModel>, WalletStorageError>;

    fn stealth_outputs_get_locked_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
    ) -> Result<Vec<StealthOutputModel>, WalletStorageError>;
    fn stealth_outputs_get_by_commitment(
        &mut self,
        resource_address: &ResourceAddress,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<StealthOutputModel, WalletStorageError>;

    // Output Locks
    fn locks_get_by_transaction_id(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<Vec<WalletLockId>, WalletStorageError>;

    // Non fungible tokens
    fn non_fungible_token_get_by_nft_id(
        &mut self,
        resource_address: ResourceAddress,
        nft_id: NonFungibleId,
    ) -> Result<NonFungibleToken, WalletStorageError>;

    fn non_fungible_token_get_ids_by_vault_id(
        &mut self,
        vault_id: &VaultId,
        limit: u64,
        offset: u64,
    ) -> Result<HashSet<NonFungibleId>, WalletStorageError>;

    fn non_fungible_token_get_all(
        &mut self,
        account: ComponentAddress,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<NonFungibleToken>, WalletStorageError>;

    fn non_fungible_token_get_resource_address(
        &mut self,
        nft_id: NonFungibleId,
    ) -> Result<ResourceAddress, WalletStorageError>;

    // Webauthn registration
    fn webauthn_is_user_registered(&mut self, username: &str) -> Result<bool, WalletStorageError>;
    fn webauthn_reg_fetch_passkeys(&mut self, username: String) -> Result<Vec<Passkey>, WalletStorageError>;

    // Authored templates
    fn authored_templates_exists_by_address(&mut self, address: &TemplateAddress) -> Result<bool, WalletStorageError>;
    fn authored_templates_fetch_by_public_key(
        &mut self,
        author_public_key: &RistrettoPublicKeyBytes,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<AuthoredTemplateModel>, u64), WalletStorageError>;

    fn shard_state_version_get(
        &mut self,
        account: &ComponentAddress,
        resource: &ResourceAddress,
    ) -> Result<HashMap<Shard, StateVersion>, WalletStorageError>;

    fn utxo_process_queue_fetch_batch(
        &mut self,
        batch_size: usize,
    ) -> Result<HashMap<ResourceAddress, HashMap<TagAndPublicNoncePair, u64>>, WalletStorageError>;
}

pub type TagAndPublicNoncePair = (UtxoTag, RistrettoPublicKeyBytes);

pub trait WalletStoreWriter {
    fn commit(self) -> Result<(), WalletStorageError>;
    fn rollback(self) -> Result<(), WalletStorageError>;

    // JWT
    fn jwt_add_empty_token(&mut self) -> Result<u64, WalletStorageError>;
    fn jwt_store_decision(&mut self, id: u64, permissions_token: Option<&str>) -> Result<(), WalletStorageError>;
    fn jwt_is_revoked(&mut self, token: &str) -> Result<bool, WalletStorageError>;
    fn jwt_revoke(&mut self, token_id: i32) -> Result<(), WalletStorageError>;

    // Key manager
    fn key_manager_insert_or_ignore(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError>;
    fn key_manager_set_active_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError>;
    fn key_manager_reset_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError>;

    // Config
    fn config_set<T: serde::Serialize + ?Sized>(
        &mut self,
        key: &str,
        value: &T,
        is_encrypted: bool,
    ) -> Result<(), WalletStorageError>;

    // Transactions
    fn transactions_insert(
        &mut self,
        transaction: &Transaction,
        new_account_info: Option<&NewAccountData>,
        is_dry_run: bool,
    ) -> Result<(), WalletStorageError>;
    fn transactions_update(&mut self, update: WalletTransactionUpdate<'_>) -> Result<(), WalletStorageError>;

    // Substates
    fn substates_upsert_root(
        &mut self,
        substate_id: VersionedSubstateIdRef<'_>,
        referenced_substates: HashSet<SubstateId>,
        module_name: Option<String>,
        template_addr: Option<TemplateAddress>,
    ) -> Result<(), WalletStorageError>;
    fn substates_upsert_child(
        &mut self,
        parent: &SubstateId,
        address: VersionedSubstateIdRef<'_>,
        referenced_substates: HashSet<SubstateId>,
    ) -> Result<(), WalletStorageError>;
    fn substates_remove(&mut self, substate: &SubstateId) -> Result<SubstateModel, WalletStorageError>;

    // Accounts
    fn accounts_set_default(&mut self, account_addr: &ComponentAddress) -> Result<(), WalletStorageError>;
    fn accounts_insert(
        &mut self,
        account_name: Option<&str>,
        account_addr: &ComponentAddress,
        owner_key_index: u64,
        is_confirmed_on_chain: bool,
        is_default: bool,
    ) -> Result<(), WalletStorageError>;

    fn accounts_update(
        &mut self,
        account_addr: &ComponentAddress,
        update: AccountUpdate,
    ) -> Result<(), WalletStorageError>;

    fn accounts_add_stealth_resource(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: ResourceAddress,
    ) -> Result<(), WalletStorageError>;

    // Vaults
    fn vaults_insert(&mut self, vault: VaultModel) -> Result<(), WalletStorageError>;
    fn vaults_update(
        &mut self,
        vault_id: VaultId,
        revealed_balance: Amount,
        confidential_balance: Amount,
    ) -> Result<(), WalletStorageError>;
    fn vaults_lock_revealed_funds(
        &mut self,
        lock_id: WalletLockId,
        vault_id: &VaultId,
        amount_to_lock: Amount,
    ) -> Result<(), WalletStorageError>;
    fn vaults_finalized_locked_revealed_funds(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    fn vaults_release_lock_revealed_funds(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    // Resources
    fn resources_upsert(&mut self, address: &ResourceAddress, resource: &Resource) -> Result<(), WalletStorageError>;
    // Confidential Outputs
    fn outputs_lock_smallest_amount(
        &mut self,
        vault_id: &VaultId,
        lock_id: WalletLockId,
    ) -> Result<ConfidentialOutputModel, WalletStorageError>;
    fn outputs_insert(&mut self, output: ConfidentialOutputModel) -> Result<(), WalletStorageError>;
    /// Mark outputs as finalized
    fn outputs_finalize_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    /// Release outputs that were locked and remove pending unconfirmed outputs for this proof
    fn outputs_release_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;

    // Stealth Outputs
    fn stealth_outputs_lock_smallest_amount(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
        lock_id: WalletLockId,
    ) -> Result<StealthOutputModel, WalletStorageError>;
    fn stealth_outputs_insert(&mut self, output: &StealthOutputModel) -> Result<(), WalletStorageError>;
    fn stealth_outputs_mark_as_spent(
        &mut self,
        resource_address: &ResourceAddress,
        id: &UtxoId,
    ) -> Result<(), WalletStorageError>;
    /// Mark outputs locked by this lock id as finalized
    fn stealth_outputs_finalize_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    /// Release outputs that were locked and remove pending unconfirmed outputs for this lock
    fn stealth_outputs_release_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    fn stealth_outputs_update(
        &mut self,
        address: &UtxoAddress,
        is_burnt: Option<bool>,
        status: Option<OutputStatus>,
        is_frozen: Option<bool>,
    ) -> Result<(), WalletStorageError>;

    // Locks
    fn locks_create(&mut self) -> Result<WalletLockId, WalletStorageError>;

    fn locks_delete(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    fn locks_link_transaction(
        &mut self,
        lock_id: WalletLockId,
        transaction_id: TransactionId,
    ) -> Result<(), WalletStorageError>;

    // Non fungible tokens
    fn non_fungible_token_upsert(&mut self, non_fungible_token: &NonFungibleToken) -> Result<(), WalletStorageError>;
    fn non_fungible_token_remove(
        &mut self,
        vault_id: &VaultId,
        non_fungible_id: &NonFungibleId,
    ) -> Result<(), WalletStorageError>;

    // Webauthn registrations
    fn webauthn_reg_insert(&mut self, username: String, passkey: Passkey) -> Result<(), WalletStorageError>;

    // Authored templates
    fn authored_templates_insert(&mut self, model: AuthoredTemplateModel) -> Result<(), WalletStorageError>;
    fn shard_state_version_set_many<I: IntoIterator<Item = (Shard, StateVersion)>>(
        &mut self,
        account: &ComponentAddress,
        resource_address: &ResourceAddress,
        shard_state_versions: I,
    ) -> Result<(), WalletStorageError>;

    fn utxo_process_queue_extend<I: IntoIterator<Item = (u64, UtxoUnspent)>>(
        &mut self,
        resource_address: &ResourceAddress,
        items: I,
    ) -> Result<(), WalletStorageError>;
    fn utxo_process_queue_remove_item(
        &mut self,
        resource_address: ResourceAddress,
        tag: UtxoTag,
        public_nonce: RistrettoPublicKeyBytes,
    ) -> Result<(), WalletStorageError>;
}
