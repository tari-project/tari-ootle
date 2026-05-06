//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, time::Duration};

use tari_engine_types::{
    resource::Resource,
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{Epoch, StateVersion, VersionedSubstateIdRef, shard::Shard};
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_template_lib::types::{
    Amount,
    ComponentAddress,
    NonFungibleId,
    ResourceAddress,
    TemplateAddress,
    UtxoAddress,
    UtxoId,
    VaultId,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
};
use webauthn_rs::prelude::Passkey;

use crate::{
    models::{
        AccountUpdate,
        AddressBookEntry,
        AuthoredTemplateModel,
        ConfidentialOutputModel,
        ImportedKeyId,
        KeyId,
        KeyType,
        NewApiKeyModel,
        NewAccountData,
        NonFungibleToken,
        OutputStatus,
        StealthOutputModel,
        SubstateModel,
        UtxoUnspent,
        VaultModel,
        WalletEvent,
        WalletLockId,
        WalletTransactionUpdate,
    },
    storage::{CommittableStore, WalletStorageError},
};

pub trait WalletStoreWriter: CommittableStore {
    // Key manager
    fn key_manager_insert_or_ignore(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError>;
    fn key_manager_set_active_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError>;
    fn key_manager_reset_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError>;
    fn key_manager_insert_imported_key(
        &mut self,
        label: &str,
        encrypted_key: &[u8],
        key_type: KeyType,
    ) -> Result<ImportedKeyId, WalletStorageError>;

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
        view_only_key_id: KeyId,
        owner_key_id: Option<KeyId>,
        owner_public_key: &RistrettoPublicKeyBytes,
        associated_stealth_resources: &HashSet<ResourceAddress>,
        birthday_epoch: Epoch,
        is_confirmed_on_chain: bool,
        is_default: bool,
    ) -> Result<(), WalletStorageError>;

    fn accounts_update(
        &mut self,
        account_addr: &ComponentAddress,
        update: AccountUpdate<'_>,
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
    fn confidential_outputs_lock_smallest_amount(
        &mut self,
        vault_id: &VaultId,
        lock_id: WalletLockId,
    ) -> Result<ConfidentialOutputModel, WalletStorageError>;
    fn confidential_outputs_insert(&mut self, output: ConfidentialOutputModel) -> Result<(), WalletStorageError>;
    /// Mark outputs as finalized
    fn confidential_outputs_finalize_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    /// Release outputs that were locked and remove pending unconfirmed outputs for this proof
    fn confidential_outputs_release_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;

    // Stealth Outputs
    fn stealth_outputs_lock_smallest_amount(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
        lock_id: WalletLockId,
    ) -> Result<StealthOutputModel, WalletStorageError>;

    fn stealth_outputs_lock_many(
        &mut self,
        resource_address: &ResourceAddress,
        utxos: &[&PedersenCommitmentBytes],
        lock_id: WalletLockId,
    ) -> Result<(), WalletStorageError>;
    fn stealth_outputs_insert(&mut self, output: &StealthOutputModel) -> Result<(), WalletStorageError>;
    fn stealth_outputs_mark_as_spent(
        &mut self,
        resource_address: &ResourceAddress,
        id: &UtxoId,
    ) -> Result<(), WalletStorageError>;
    fn stealth_outputs_update(
        &mut self,
        address: &UtxoAddress,
        is_burnt: Option<bool>,
        status: Option<OutputStatus>,
        is_frozen: Option<bool>,
    ) -> Result<(), WalletStorageError>;

    // Locks
    fn locks_create(&mut self, timeout: Option<Duration>) -> Result<WalletLockId, WalletStorageError>;

    fn locks_delete(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    fn locks_link_transaction(
        &mut self,
        lock_id: WalletLockId,
        transaction_id: TransactionId,
    ) -> Result<(), WalletStorageError>;

    fn locks_release_stale(&mut self) -> Result<usize, WalletStorageError>;

    /// Release the lock including all outputs and vaults that were locked. Release is used when a transaction is
    /// aborted.
    fn locks_release(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError>;
    /// Finalize the lock according to the provided diff. Any outputs and vaults locked by this lock and included in the
    /// diff are finalised (marked as unspent/funds removed/added as necessary). Any objects not included in the diff
    /// are reverted and released from the lock. This is used when a transaction is committed.
    fn locks_unlock_finalized(&mut self, lock_id: WalletLockId, diff: &SubstateDiff) -> Result<(), WalletStorageError>;

    // Non fungible tokens
    fn non_fungible_token_upsert(&mut self, non_fungible_token: &NonFungibleToken) -> Result<(), WalletStorageError>;
    fn non_fungible_token_remove(
        &mut self,
        vault_id: &VaultId,
        non_fungible_id: &NonFungibleId,
    ) -> Result<(), WalletStorageError>;

    // Webauthn registrations
    fn webauthn_reg_insert(&mut self, username: String, passkey: Passkey) -> Result<(), WalletStorageError>;

    // API keys
    fn api_keys_insert(&mut self, api_key: NewApiKeyModel) -> Result<(), WalletStorageError>;
    fn api_keys_touch_last_used(&mut self, id: &str) -> Result<(), WalletStorageError>;
    fn api_keys_revoke(&mut self, id: &str) -> Result<(), WalletStorageError>;

    // Authored templates
    fn authored_templates_insert(&mut self, model: AuthoredTemplateModel) -> Result<(), WalletStorageError>;
    fn shard_state_version_set_many<I: IntoIterator<Item = (Shard, StateVersion)>>(
        &mut self,
        account: &ComponentAddress,
        resource_address: &ResourceAddress,
        shard_state_versions: I,
    ) -> Result<(), WalletStorageError>;

    fn utxo_process_queue_extend<I: IntoIterator<Item = (ComponentAddress, UtxoUnspent)>>(
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

    // Address book
    fn address_book_insert(
        &mut self,
        name: &str,
        address: &str,
        note: Option<&str>,
    ) -> Result<AddressBookEntry, WalletStorageError>;
    fn address_book_update(
        &mut self,
        name: &str,
        new_name: Option<&str>,
        address: Option<&str>,
        note: Option<&str>,
    ) -> Result<AddressBookEntry, WalletStorageError>;
    fn address_book_delete(&mut self, name: &str) -> Result<(), WalletStorageError>;
}

pub trait WalletEventStoreWriter {
    fn append_wallet_event(&mut self, event: &WalletEvent) -> Result<(), WalletStorageError>;
}
