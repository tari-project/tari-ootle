//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, HashSet};

use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{StateVersion, shard::Shard, substate_type::SubstateType};
use tari_ootle_transaction::TransactionId;
use tari_template_lib::types::{
    ComponentAddress,
    NonFungibleId,
    ResourceAddress,
    ResourceType,
    TemplateAddress,
    VaultId,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
};
use webauthn_rs::prelude::Passkey;

use crate::{
    models::{
        Account,
        AddressBookEntry,
        ApiKeyModel,
        AuthoredTemplateModel,
        ConfidentialOutputModel,
        Config,
        KeyType,
        NonFungibleToken,
        OutputStatus,
        ResourceModel,
        StealthBalance,
        StealthOutputInfo,
        StealthOutputModel,
        SubstateModel,
        TransactionStatus,
        VaultModel,
        WalletLockId,
        WalletTransaction,
    },
    storage::{TagAndPublicNoncePair, WalletStorageError},
};

pub trait WalletStoreReader {
    // Key manager
    fn key_manager_get_all(&mut self, branch: &str) -> Result<Vec<(u64, bool)>, WalletStorageError>;
    fn key_manager_get_active_index(&mut self, branch: &str) -> Result<u64, WalletStorageError>;
    fn key_manager_get_last_index(&mut self, branch: &str) -> Result<u64, WalletStorageError>;
    fn key_manager_get_raw_imported_key(&mut self, id: u64) -> Result<(KeyType, Box<[u8]>), WalletStorageError>;
    // Config
    fn config_get<T: serde::de::DeserializeOwned>(&mut self, key: &str) -> Result<Config<T>, WalletStorageError>;
    fn config_get_string(&mut self, key: &str) -> Result<Config<String>, WalletStorageError>;
    fn config_exists(&mut self, key: &str) -> Result<bool, WalletStorageError>;
    // Transactions
    fn transactions_get(&mut self, transaction_id: TransactionId) -> Result<WalletTransaction, WalletStorageError>;
    /// Read the *full* transaction (with blob payloads) — needed for re-submission and other
    /// operations that require the original bytes. The plain `transactions_get` returns the
    /// pruned, API-facing form.
    fn transactions_get_full(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<tari_ootle_transaction::Transaction, WalletStorageError>;
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
    fn accounts_get_many(&mut self, offset: usize, limit: usize) -> Result<Vec<Account>, WalletStorageError>;
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
    fn vaults_get_ids_by_account(
        &mut self,
        account_addr: &ComponentAddress,
    ) -> Result<Vec<VaultId>, WalletStorageError>;

    // Resources
    fn resources_get(&mut self, resource_address: &ResourceAddress) -> Result<ResourceModel, WalletStorageError>;
    fn resources_get_by_type(&mut self, resource_type: ResourceType) -> Result<Vec<ResourceModel>, WalletStorageError>;
    fn resources_get_many<'a, I: IntoIterator<Item = &'a ResourceAddress>>(
        &mut self,
        addresses: I,
    ) -> Result<Vec<ResourceModel>, WalletStorageError>;

    // Confidential Outputs
    fn confidential_outputs_get_unspent_balance(&mut self, vault_id: &VaultId) -> Result<u64, WalletStorageError>;
    fn confidential_outputs_get_locked_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
    ) -> Result<Vec<ConfidentialOutputModel>, WalletStorageError>;
    fn confidential_outputs_get_by_commitment(
        &mut self,
        vault_id: &VaultId,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<ConfidentialOutputModel, WalletStorageError>;

    fn confidential_outputs_get_by_account_and_status(
        &mut self,
        account_addr: &ComponentAddress,
        status: OutputStatus,
    ) -> Result<Vec<ConfidentialOutputModel>, WalletStorageError>;

    // Stealth outputs
    fn stealth_outputs_get_unspent_balance(
        &mut self,
        resource_address: &ResourceAddress,
    ) -> Result<StealthBalance, WalletStorageError>;

    fn stealth_outputs_count_by_status(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
        status: OutputStatus,
    ) -> Result<u64, WalletStorageError>;

    fn stealth_outputs_get_unspent_by_account(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: Option<&ResourceAddress>,
        exclude_locked: bool,
    ) -> Result<Vec<StealthOutputInfo>, WalletStorageError>;

    fn stealth_outputs_get_unspent_for_spending(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
        lock_id: WalletLockId,
    ) -> Result<Vec<StealthOutputInfo>, WalletStorageError>;

    fn stealth_outputs_get_locked_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
    ) -> Result<Vec<StealthOutputModel>, WalletStorageError>;
    fn stealth_outputs_get_by_commitment(
        &mut self,
        resource_address: &ResourceAddress,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<StealthOutputModel, WalletStorageError>;

    fn stealth_outputs_get_many(
        &mut self,
        resource_address: &ResourceAddress,
        by_account: Option<&ComponentAddress>,
        by_status: Option<OutputStatus>,
    ) -> Result<Vec<StealthOutputModel>, WalletStorageError>;

    // Output Locks
    fn locks_get_by_transaction_id(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<WalletLockId, WalletStorageError>;

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

    // API keys
    fn api_keys_get(&mut self, id: &str) -> Result<ApiKeyModel, WalletStorageError>;
    fn api_keys_get_active(&mut self, id: &str) -> Result<ApiKeyModel, WalletStorageError>;
    fn api_keys_list_active(&mut self) -> Result<Vec<ApiKeyModel>, WalletStorageError>;

    // Authored templates
    fn authored_templates_exists_by_address(&mut self, address: &TemplateAddress) -> Result<bool, WalletStorageError>;
    fn authored_templates_get_by_address(
        &mut self,
        address: &TemplateAddress,
    ) -> Result<AuthoredTemplateModel, WalletStorageError>;
    fn authored_templates_get_many(
        &mut self,
        author_public_key: Option<&RistrettoPublicKeyBytes>,
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
    ) -> Result<HashMap<ResourceAddress, HashMap<TagAndPublicNoncePair, ComponentAddress>>, WalletStorageError>;

    // Address book
    fn address_book_get(&mut self, name: &str) -> Result<AddressBookEntry, WalletStorageError>;
    fn address_book_get_all(&mut self) -> Result<Vec<AddressBookEntry>, WalletStorageError>;
}
