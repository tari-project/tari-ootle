//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    iter,
    ops::{Deref, DerefMut},
    str::FromStr,
    sync::MutexGuard,
    time::Duration,
};

use diesel::{
    BoolExpressionMethods,
    JoinOnDsl,
    NullableExpressionMethods,
    OptionalExtension,
    QueryDsl,
    RunQueryDsl,
    SqliteConnection,
    dsl,
};
use log::*;
use serde::Serialize;
use tari_engine_types::{
    resource::Resource,
    substate::{SubstateDiff, SubstateId},
};
use tari_ootle_common_types::{
    Epoch,
    StateVersion,
    VersionedSubstateIdRef,
    displayable::Displayable,
    optional::Optional,
    shard::Shard,
};
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_ootle_wallet_sdk::{
    models::{
        AccountUpdate,
        AddressBookEntry,
        ApiKey,
        AuthoredTemplateModel,
        BalanceChange,
        BalanceChangeSource,
        ConfidentialOutputModel,
        ImportedKeyId,
        KeyId,
        KeyType,
        NewAccountData,
        NonFungibleToken,
        OutputStatus,
        StealthOutputModel,
        SubstateModel,
        TransactionStatus,
        UtxoUnspent,
        VaultModel,
        WalletEvent,
        WalletLockId,
        WalletTransactionUpdate,
    },
    storage::{CommittableStore, WalletEventStoreWriter, WalletStorageError, WalletStoreReader, WalletStoreWriter},
};
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    EncryptedData,
    NonFungibleId,
    ResourceAddress,
    TemplateAddress,
    UtxoAddress,
    UtxoId,
    VaultId,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
};
use tari_utilities::hex::Hex;
use time::PrimitiveDateTime;
use webauthn_rs::prelude::Passkey;

use crate::{
    diesel::ExpressionMethods,
    models,
    models::{AddressBookEntryChangeset, StealthOutputUpdate},
    reader::ReadTransaction,
    serialization::{deserialize_hex_try_from, deserialize_json, serialize_hex, serialize_json},
};

const LOG_TARGET: &str = "auth::tari::dan::wallet_sdk::storage_sqlite::writer";

pub struct WriteTransaction<'a> {
    /// In SQLite any transaction is writable. We keep a ReadTransaction to satisfy the Deref requirement of the
    /// WalletStore.
    transaction: ReadTransaction<'a>,
}

impl<'a> WriteTransaction<'a> {
    pub fn new(connection: MutexGuard<'a, SqliteConnection>) -> Self {
        Self {
            transaction: ReadTransaction::new(connection),
        }
    }

    fn ensure_lock_exists(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        use crate::schema::locks;

        let count = locks::table
            .filter(locks::id.eq(lock_id))
            .limit(1)
            .count()
            .get_result::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("ensure_lock_exists", e))?;
        if count == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "ensure_lock_exists",
                entity: "lock".to_string(),
                key: lock_id.to_string(),
            });
        }
        Ok(())
    }

    fn stealth_outputs_release_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_release_by_lock_id";
        use crate::schema::stealth_outputs;

        // Unlock locked unspent stealth_outputs
        diesel::update(stealth_outputs::table)
            .filter(stealth_outputs::lock_id.eq(lock_id))
            .filter(stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .filter(stealth_outputs::is_on_chain.eq(true))
            .set((
                stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                stealth_outputs::lock_id.eq::<Option<i32>>(None),
                stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        // Remove stealth_outputs that were created by this lock
        diesel::delete(stealth_outputs::table)
            .filter(stealth_outputs::lock_id.eq(lock_id))
            .filter(
                stealth_outputs::status
                    .eq(OutputStatus::LockedUnconfirmed.as_key_str())
                    .or(stealth_outputs::status
                        .eq(OutputStatus::LockedForSpend.as_key_str())
                        .and(stealth_outputs::is_on_chain.eq(false))),
            )
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn stealth_outputs_finalize_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
        diff: &SubstateDiff,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_finalize_by_lock_id";
        use crate::schema::stealth_outputs;

        // Fetch the outputs locked by this lock_id
        let locked_outputs = stealth_outputs::table
            .select((
                stealth_outputs::id,
                stealth_outputs::resource_address,
                stealth_outputs::commitment,
            ))
            .filter(stealth_outputs::lock_id.eq(lock_id))
            .load_iter::<(i32, String, String), _>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let up_id_index = diff
            .up_iter()
            .filter_map(|(id, _)| id.as_utxo_address())
            .collect::<HashSet<_>>();
        let down_id_index = diff
            .down_iter()
            .filter_map(|(id, _)| id.as_utxo_address())
            .collect::<HashSet<_>>();
        let mut to_confirm = vec![];
        let mut to_spend = vec![];

        for res in locked_outputs {
            let (id, resx, commitment) = res.map_err(|e| WalletStorageError::general(OPERATION, e))?;
            let resource_address = resx.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_to_substate_id",
                item: "output",
                details: format!("Corrupt db: invalid resource address '{resx}' for id {id}"),
            })?;
            let commitment: PedersenCommitmentBytes =
                deserialize_hex_try_from(&commitment).map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output commitment",
                    details: "Corrupt db: invalid hex representation".to_string(),
                })?;

            let addr = UtxoAddress::new(resource_address, commitment.into());
            let is_downed = down_id_index.contains(&addr);
            let is_upped = up_id_index.contains(&addr);

            if is_upped {
                to_confirm.push(id);
            } else if is_downed {
                to_spend.push(id);
            } else {
                // Lock will be released (i.e. LockedUnconfirmed outputs deleted, LockedForSpend -> Unspent)
            }
        }

        if !to_confirm.is_empty() {
            // Unlock locked unconfirmed stealth_outputs
            diesel::update(stealth_outputs::table)
                .filter(stealth_outputs::lock_id.eq(lock_id))
                .filter(stealth_outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
                .filter(stealth_outputs::id.eq_any(to_confirm))
                .set((
                    stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                    stealth_outputs::lock_id.eq::<Option<i32>>(None),
                    stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
                    stealth_outputs::is_on_chain.eq(true),
                ))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }

        if !to_spend.is_empty() {
            // Mark locked outputs as spent
            diesel::update(stealth_outputs::table)
                .filter(stealth_outputs::lock_id.eq(lock_id))
                .filter(stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
                .filter(stealth_outputs::id.eq_any(to_spend))
                .set((
                    stealth_outputs::status.eq(OutputStatus::Spent.as_key_str()),
                    stealth_outputs::lock_id.eq::<Option<i32>>(None),
                    stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
                ))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }

        // Any outputs that were not confirmed or spent are released
        self.stealth_outputs_release_by_lock_id(lock_id)?;

        Ok(())
    }
}

impl CommittableStore for WriteTransaction<'_> {
    fn commit(&mut self) -> Result<(), WalletStorageError> {
        self.transaction.commit_internal()?;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), WalletStorageError> {
        self.transaction.rollback_internal()?;
        Ok(())
    }
}

impl WalletStoreWriter for WriteTransaction<'_> {
    // -------------------------------- KeyManager -------------------------------- //

    fn key_manager_insert_or_ignore(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
        use crate::schema::key_manager_states;
        let index =
            i64::try_from(index).map_err(|_| WalletStorageError::general("key_manager_insert", "index is negative"))?;
        let count = key_manager_states::table
            .select(key_manager_states::id)
            .filter(key_manager_states::branch_seed.eq(branch))
            .limit(1)
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_insert", e))?;

        // Set active if this is the only key branch
        let is_active = count == 0;

        let value_set = (
            key_manager_states::branch_seed.eq(branch),
            key_manager_states::index.eq(index),
            key_manager_states::is_active.eq(is_active),
        );

        diesel::insert_into(key_manager_states::table)
            .values(value_set)
            .on_conflict_do_nothing()
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_insert", e))?;

        Ok(())
    }

    fn key_manager_set_active_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
        use crate::schema::key_manager_states;
        let index = i64::try_from(index)
            .map_err(|_| WalletStorageError::general("key_manager_set_active_index", "index too large"))?;

        // Ensure it exists
        self.key_manager_insert_or_ignore(branch, index as u64)?;

        let active_id = key_manager_states::table
            .select(key_manager_states::id)
            .filter(key_manager_states::branch_seed.eq(branch))
            .filter(key_manager_states::index.eq(index))
            .limit(1)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("key_manager_set_active_index", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "key_manager_set_active_index",
                entity: "key_manager_states".to_string(),
                key: format!("branch = {}, index = {}", branch, index),
            })?;

        diesel::update(key_manager_states::table)
            .set((
                key_manager_states::is_active.eq(false),
                key_manager_states::updated_at.eq(diesel::dsl::now),
            ))
            .filter(key_manager_states::branch_seed.eq(branch))
            .filter(key_manager_states::is_active.eq(true))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_set_active_index", e))?;

        diesel::update(key_manager_states::table)
            .set((
                key_manager_states::is_active.eq(true),
                key_manager_states::updated_at.eq(diesel::dsl::now),
            ))
            .filter(key_manager_states::id.eq(active_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_set_active_index", e))?;

        Ok(())
    }

    fn key_manager_reset_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "key_manager_reset_index";
        use crate::schema::key_manager_states;
        let index = i64::try_from(index).map_err(|_| WalletStorageError::general(OPERATION, "index too large"))?;

        diesel::delete(key_manager_states::table)
            .filter(key_manager_states::branch_seed.eq(branch))
            .filter(key_manager_states::index.gt(index))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn key_manager_insert_imported_key(
        &mut self,
        label: &str,
        encrypted_key: &[u8],
        key_type: KeyType,
    ) -> Result<ImportedKeyId, WalletStorageError> {
        const OPERATION: &str = "key_manager_insert_imported_key";
        use crate::schema::key_manager_imported_keys;

        diesel::insert_into(key_manager_imported_keys::table)
            .values((
                key_manager_imported_keys::label.eq(label),
                key_manager_imported_keys::encrypted_secret.eq(encrypted_key),
                key_manager_imported_keys::key_type.eq(key_type.to_string()),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        let last_inserted_id: i32 = diesel::select(dsl::sql::<diesel::sql_types::Integer>("last_insert_rowid()"))
            .get_result(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(ImportedKeyId::from(last_inserted_id as u32))
    }

    // -------------------------------- Config -------------------------------- //

    fn config_set<T: Serialize + ?Sized>(
        &mut self,
        key: &str,
        value: &T,
        is_encrypted: bool,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::config;

        let exists = config::table
            .filter(config::key.eq(key))
            .limit(1)
            .count()
            .get_result(self.connection())
            .map(|count: i64| count > 0)
            .map_err(|e| WalletStorageError::general("key_manager_set_index", e))?;

        if exists {
            diesel::update(config::table)
                .set((
                    // TODO: we should store bytes to allow for encrypted values with the downside of not being able to
                    // "see" the JSON Or we could have a cleartext string column, and an encrypted
                    // bytes column
                    config::value.eq(serialize_json(value)?),
                    config::is_encrypted.eq(is_encrypted),
                    config::updated_at.eq(diesel::dsl::now),
                ))
                .filter(config::key.eq(key))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general("key_manager_set_index", e))?;
        } else {
            diesel::insert_into(config::table)
                .values((
                    config::key.eq(key),
                    config::value.eq(serialize_json(value)?),
                    config::is_encrypted.eq(is_encrypted),
                ))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general("key_manager_set_index", e))?;
        }

        Ok(())
    }

    // -------------------------------- Transactions -------------------------------- //
    fn transactions_insert(
        &mut self,
        transaction: &Transaction,
        new_account_info: Option<&NewAccountData>,
        linked_accounts: &[ComponentAddress],
        is_dry_run: bool,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::{accounts, transaction_accounts, transactions};

        let transaction_id = serialize_hex(transaction.calculate_id());
        let ref_components = transaction
            .as_referenced_components()
            .map(|c| c.to_string())
            .collect::<Vec<_>>();
        let signers = transaction
            .signatures()
            .iter()
            .map(|s| s.public_key())
            .chain(iter::once(transaction.seal_signature().public_key()))
            .collect::<Vec<_>>();
        diesel::insert_into(transactions::table)
            .values((
                transactions::transaction_id.eq(transaction_id.as_str()),
                transactions::transaction_json.eq(serialize_json(transaction)?),
                transactions::referenced_components.eq(serialize_json(&ref_components)?),
                transactions::signers.eq(serialize_json(&signers)?),
                transactions::status.eq(TransactionStatus::New.as_key_str()),
                transactions::new_account_info.eq(new_account_info.map(serialize_json).transpose()?),
                transactions::dry_run.eq(is_dry_run),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("transactions_insert", e))?;

        // Link the transaction to the wallet account(s) it involves so the list can be filtered
        // per account. Dry-run transactions are never surfaced in the list, so they are not linked.
        if !is_dry_run {
            let mut seen = HashSet::new();
            for account_address in linked_accounts {
                let address = account_address.to_string();
                if !seen.insert(address.clone()) {
                    continue;
                }
                let account_id: Option<i32> = accounts::table
                    .select(accounts::id)
                    .filter(accounts::address.eq(&address))
                    .first(self.connection())
                    .optional()
                    .map_err(|e| WalletStorageError::general("transactions_insert account lookup", e))?;
                // The account may not be known to this wallet (e.g. an external recipient); skip it.
                let Some(account_id) = account_id else {
                    continue;
                };
                diesel::insert_into(transaction_accounts::table)
                    .values((
                        transaction_accounts::transaction_id.eq(transaction_id.as_str()),
                        transaction_accounts::account_id.eq(account_id),
                    ))
                    .execute(self.connection())
                    .map_err(|e| WalletStorageError::general("transactions_insert link account", e))?;
            }
        }

        Ok(())
    }

    fn transactions_update(&mut self, update: WalletTransactionUpdate<'_>) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "transactions_update";
        use crate::schema::transactions;

        let num_rows = diesel::update(transactions::table)
            .set((
                transactions::result.eq(update.result.map(serialize_json).transpose()?),
                transactions::status.eq(update.new_status.as_key_str()),
                transactions::final_fee.eq(update.final_fee.map(|v| v as i64)),
                transactions::qcs.eq(update.qcs.map(serialize_json).transpose()?),
                transactions::executed_time_ms.eq(update
                    .execution_time
                    .map(|v| i64::try_from(v.as_millis()).unwrap_or(i64::MAX))),
                transactions::finalized_time.eq(update.finalized_time),
                transactions::invalid_reason.eq(update.invalid_reason),
                transactions::updated_at.eq(diesel::dsl::now),
            ))
            .filter(transactions::transaction_id.eq(update.transaction_id.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "transaction".to_string(),
                key: update.transaction_id.to_string(),
            });
        }

        Ok(())
    }

    // -------------------------------- Substates -------------------------------- //
    fn substates_upsert_root(
        &mut self,
        substate_id: VersionedSubstateIdRef<'_>,
        referenced_substates: HashSet<SubstateId>,
        module_name: Option<String>,
        template_addr: Option<TemplateAddress>,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::substates;

        diesel::insert_into(substates::table)
            .values((
                substates::address.eq(substate_id.substate_id().to_string()),
                substates::module_name.eq(&module_name),
                substates::template_address.eq(template_addr.map(|a| a.to_string())),
                substates::referenced_substates.eq(serialize_json(&referenced_substates)?),
                substates::version.eq(substate_id.version() as i32),
            ))
            .on_conflict(substates::address)
            .do_update()
            .set((
                substates::module_name.eq(&module_name),
                substates::template_address.eq(template_addr.map(|a| a.to_string())),
                substates::referenced_substates.eq(serialize_json(&referenced_substates)?),
                substates::version.eq(substate_id.version() as i32),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("substates_upsert_root", e))?;

        Ok(())
    }

    fn substates_upsert_child(
        &mut self,
        parent: &SubstateId,
        address: VersionedSubstateIdRef<'_>,
        referenced_substates: HashSet<SubstateId>,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::substates;

        diesel::insert_into(substates::table)
            .values((
                substates::address.eq(address.substate_id().to_string()),
                substates::parent_address.eq(Some(parent.to_string())),
                substates::referenced_substates.eq(serialize_json(&referenced_substates)?),
                substates::version.eq(address.version() as i32),
            ))
            .on_conflict(substates::address)
            .do_update()
            .set((
                substates::parent_address.eq(Some(parent.to_string())),
                substates::referenced_substates.eq(serialize_json(&referenced_substates)?),
                substates::version.eq(address.version() as i32),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("substates_upsert_child", e))?;

        Ok(())
    }

    fn substates_remove(&mut self, substate_addr: &SubstateId) -> Result<SubstateModel, WalletStorageError> {
        use crate::schema::substates;

        let substate = self.transaction.substates_get(substate_addr)?;
        let num_rows = diesel::delete(substates::table)
            .filter(substates::address.eq(substate_addr.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("substates_remove", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "substates_remove",
                entity: "substate".to_string(),
                key: substate.substate_id.to_string(),
            });
        }

        Ok(substate)
    }

    // -------------------------------- Accounts -------------------------------- //

    fn accounts_set_default(&mut self, address: &ComponentAddress) -> Result<(), WalletStorageError> {
        use crate::schema::accounts;

        diesel::update(accounts::table)
            .set(accounts::is_default.eq(false))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_set_default clear previous default", e))?;

        let num_rows = diesel::update(accounts::table)
            .set(accounts::is_default.eq(true))
            .filter(accounts::address.eq(address.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_set_default", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "accounts_set_default",
                entity: "account".to_string(),
                key: address.to_string(),
            });
        }

        Ok(())
    }

    fn accounts_insert(
        &mut self,
        account_name: Option<&str>,
        address: &ComponentAddress,
        view_only_key_id: KeyId,
        owner_key_id: Option<KeyId>,
        owner_public_key: &RistrettoPublicKeyBytes,
        associated_stealth_resources: &HashSet<ResourceAddress>,
        birthday_epoch: Epoch,
        is_confirmed_on_chain: bool,
        is_default: bool,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::accounts;

        if is_default {
            diesel::update(accounts::table)
                .set(accounts::is_default.eq(false))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general("accounts_insert clear previous default", e))?;
        }

        diesel::insert_into(accounts::table)
            .values((
                accounts::name.eq(account_name),
                accounts::address.eq(address.to_string()),
                accounts::view_only_key_id.eq(serialize_json(&view_only_key_id)?),
                accounts::owner_key_id.eq(owner_key_id.as_ref().map(serialize_json).transpose()?),
                accounts::owner_public_key.eq(serialize_hex(owner_public_key)),
                accounts::stealth_resources.eq(serialize_json(&associated_stealth_resources)?),
                accounts::birthday_epoch.eq(birthday_epoch.as_u64() as i64),
                accounts::is_confirmed_on_chain.eq(is_confirmed_on_chain),
                accounts::is_default.eq(is_default),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_insert", e))?;

        Ok(())
    }

    fn accounts_update(
        &mut self,
        address: &ComponentAddress,
        update: AccountUpdate<'_>,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::accounts;
        let AccountUpdate {
            name,
            is_account_on_chain,
        } = update;

        if name.is_none() && is_account_on_chain.is_none() {
            // Nothing to do
            return Ok(());
        }

        let changeset = (
            name.map(|n| accounts::name.eq(n)),
            is_account_on_chain.map(|v| accounts::is_confirmed_on_chain.eq(v)),
        );

        let num_rows = diesel::update(accounts::table)
            .set(changeset)
            .filter(accounts::address.eq(address.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_update", e))?;

        if num_rows == 0 {
            // Check if the account exists, because this could have been an update that didnt change anything
            // (rows_affected = 0)
            let exists = accounts::table
                .filter(accounts::address.eq(address.to_string()))
                .limit(1)
                .count()
                .get_result::<i64>(self.connection())
                .map_err(|e| WalletStorageError::general("accounts_update", e))?;

            if exists == 0 {
                return Err(WalletStorageError::NotFound {
                    operation: "accounts_update",
                    entity: "account".to_string(),
                    key: address.to_string(),
                });
            }
        }

        Ok(())
    }

    fn accounts_add_stealth_resource(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: ResourceAddress,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "accounts_add_stealth_resource";
        use crate::schema::accounts;

        let mut resources = self.accounts_get_associated_stealth_resources(account_addr)?;
        resources.insert(resource_address);

        diesel::update(accounts::table)
            .set(accounts::stealth_resources.eq(serialize_json(&resources)?))
            .filter(accounts::address.eq(account_addr.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn vaults_insert(&mut self, vault: VaultModel) -> Result<(), WalletStorageError> {
        use crate::schema::{accounts, vaults};

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(vault.account_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_insert", e))?;

        let values = (
            vaults::account_id.eq(account_id),
            vaults::address.eq(vault.id.to_string()),
            vaults::revealed_balance.eq(vault.revealed_balance.to_string()),
            vaults::confidential_balance.eq(vault.confidential_balance.to_string()),
            vaults::resource_address.eq(vault.resource_address.to_string()),
            vaults::resource_type.eq(format!("{:?}", vault.resource_type)),
            vaults::token_symbol.eq(vault.token_symbol),
            vaults::divisibility.eq(i32::from(vault.divisibility)),
        );
        diesel::insert_into(vaults::table)
            .values(values)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_insert", e))?;

        Ok(())
    }

    fn vaults_update(
        &mut self,
        vault_id: VaultId,
        revealed_balance: Amount,
        confidential_balance: Amount,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::vaults;

        let changeset = (
            vaults::revealed_balance.eq(revealed_balance.to_string()),
            vaults::confidential_balance.eq(confidential_balance.to_string()),
        );

        let num_rows = diesel::update(vaults::table)
            .set(changeset)
            .filter(vaults::address.eq(vault_id.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_update", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "vaults_update",
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            });
        }

        Ok(())
    }

    fn balance_changes_insert(
        &mut self,
        account_address: &ComponentAddress,
        vault_address: Option<&VaultId>,
        resource_address: &ResourceAddress,
        before_revealed_balance: &Amount,
        after_revealed_balance: &Amount,
        before_confidential_balance: &Amount,
        after_confidential_balance: &Amount,
        source: &BalanceChangeSource,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "balance_changes_insert";
        use crate::schema::{account_balance_changes, accounts, vaults};

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(account_address.to_string()))
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "account".to_string(),
                key: account_address.to_string(),
            })?;

        let vault_db_id = match vault_address {
            Some(vault_addr) => {
                let vid = vaults::table
                    .select(vaults::id)
                    .filter(vaults::address.eq(vault_addr.to_string()))
                    .first::<i32>(self.connection())
                    .optional()
                    .map_err(|e| WalletStorageError::general(OPERATION, e))?
                    .ok_or_else(|| WalletStorageError::NotFound {
                        operation: OPERATION,
                        entity: "vault".to_string(),
                        key: vault_addr.to_string(),
                    })?;
                Some(vid)
            },
            None => None,
        };

        let source_str = models::balance_change_source_to_string(source);
        let transaction_id = match source {
            BalanceChangeSource::Transaction { transaction_id } => Some(transaction_id.to_string()),
            _ => None,
        };

        let values = (
            account_balance_changes::vault_id.eq(vault_db_id),
            account_balance_changes::account_id.eq(account_id),
            account_balance_changes::resource_address.eq(resource_address.to_string()),
            account_balance_changes::before_revealed_balance.eq(before_revealed_balance.to_string()),
            account_balance_changes::after_revealed_balance.eq(after_revealed_balance.to_string()),
            account_balance_changes::before_confidential_balance.eq(before_confidential_balance.to_string()),
            account_balance_changes::after_confidential_balance.eq(after_confidential_balance.to_string()),
            account_balance_changes::revealed_delta.eq(BalanceChange::compute_delta(
                *before_revealed_balance,
                *after_revealed_balance,
            )),
            account_balance_changes::confidential_delta.eq(BalanceChange::compute_delta(
                *before_confidential_balance,
                *after_confidential_balance,
            )),
            account_balance_changes::source.eq(source_str),
            account_balance_changes::transaction_id.eq(transaction_id),
        );

        diesel::insert_into(account_balance_changes::table)
            .values(values)
            .on_conflict_do_nothing()
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn balance_changes_promote_scan_to_transaction(
        &mut self,
        vault_id: &VaultId,
        transaction_id: &TransactionId,
        after_revealed_balance: &Amount,
        after_confidential_balance: &Amount,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "balance_changes_promote_scan_to_transaction";
        use crate::schema::{account_balance_changes, vaults};

        let vault_db_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            })?;

        let scan_id = account_balance_changes::table
            .select(account_balance_changes::id)
            .filter(account_balance_changes::vault_id.eq(vault_db_id))
            .filter(account_balance_changes::source.eq("Scan"))
            .filter(account_balance_changes::transaction_id.is_null())
            .filter(account_balance_changes::after_revealed_balance.eq(after_revealed_balance.to_string()))
            .filter(account_balance_changes::after_confidential_balance.eq(after_confidential_balance.to_string()))
            .order(account_balance_changes::created_at.desc())
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if let Some(id) = scan_id {
            diesel::update(account_balance_changes::table.filter(account_balance_changes::id.eq(id)))
                .set((
                    account_balance_changes::source.eq("Transaction"),
                    account_balance_changes::transaction_id.eq(Some(transaction_id.to_string())),
                ))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }

        Ok(())
    }

    fn vaults_lock_revealed_funds(
        &mut self,
        lock_id: WalletLockId,
        vault_id: &VaultId,
        amount_to_lock: Amount,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "vaults_lock_revealed_funds";
        use crate::schema::{vault_locks, vaults};

        if amount_to_lock.is_zero() {
            // No-op
            return Ok(());
        }
        if amount_to_lock.is_negative() {
            return Err(WalletStorageError::bad_query(
                OPERATION,
                "amount to lock cannot be negative",
            ));
        }

        self.ensure_lock_exists(lock_id)?;
        let vault_str = vault_id.to_string();

        let vault_db_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(&vault_str))
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "vault".to_string(),
                key: vault_str.clone(),
            })?;

        let existing_lock = vault_locks::table
            .select((vault_locks::id, vault_locks::amount))
            .filter(vault_locks::vault_id.eq(vault_db_id))
            .filter(vault_locks::lock_id.eq(lock_id))
            .first::<(i32, String)>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let amount_to_lock = amount_to_lock
            .to_u64_checked()
            .ok_or_else(|| WalletStorageError::bad_query(OPERATION, "amount to lock is too large"))?;

        if let Some((existing_lock_id, lock_amount)) = existing_lock {
            let amount = lock_amount
                .parse::<Amount>()
                .map_err(|e| WalletStorageError::DecodingError {
                    operation: OPERATION,
                    item: "lock amount",
                    details: format!(
                        "Corrupt db: invalid lock amount '{lock_amount}' for lock_id {existing_lock_id}: {e}"
                    ),
                })?;
            let amount = amount
                .checked_add(amount_to_lock.into())
                .ok_or_else(|| WalletStorageError::bad_query(OPERATION, "resulting lock amount is too large"))?;
            // Add to the existing lock
            diesel::update(vault_locks::table)
                .set(vault_locks::amount.eq(amount.to_string()))
                .filter(vault_locks::id.eq(existing_lock_id))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        } else {
            diesel::insert_into(vault_locks::table)
                .values((
                    vault_locks::vault_id.eq(vault_db_id),
                    vault_locks::lock_id.eq(lock_id),
                    vault_locks::amount.eq(amount_to_lock.to_string()),
                ))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }

        Ok(())
    }

    fn vaults_finalized_locked_revealed_funds(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "vaults_finalized_locked_revealed_funds";
        use crate::schema::{vault_locks, vaults};

        // Fetch the vault locked by this lock_id
        let (vault_id, amount, revealed_balance) = vault_locks::table
            .inner_join(vaults::table.on(vaults::id.eq(vault_locks::vault_id)))
            .select((vault_locks::vault_id, vault_locks::amount, vaults::revealed_balance))
            .filter(vault_locks::lock_id.eq(lock_id))
            .first::<(i32, String, String)>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "vault lock".to_string(),
                key: lock_id.to_string(),
            })?;
        let amount = Amount::from_str(&amount).map_err(|e| WalletStorageError::DataInconsistent {
            operation: OPERATION,
            details: format!(
                "Corrupt db: unable to convert lock amount '{amount}' to Amount for lock_id {lock_id}: {e}"
            ),
        })?;

        // Delete the lock record
        diesel::delete(vault_locks::table)
            .filter(vault_locks::lock_id.eq(lock_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let revealed_amount =
            Amount::from_str(&revealed_balance).map_err(|e| WalletStorageError::DataInconsistent {
                operation: OPERATION,
                details: format!(
                    "Corrupt db: unable to convert revealed balance '{revealed_balance}' to Amount for vault_id \
                     {vault_id}: {e}"
                ),
            })?;
        let new_balance = revealed_amount
            .checked_sub(amount)
            .ok_or_else(|| WalletStorageError::OperationError {
                operation: OPERATION,
                details: format!(
                    "Corrupt db: revealed balance {revealed_balance} is less than locked amount {amount} for vault_id \
                     {vault_id}"
                ),
            })?;

        let num_rows = diesel::update(vaults::table)
            .set(vaults::revealed_balance.eq(new_balance.to_string()))
            .filter(vaults::id.eq(vault_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "lock on vault".to_string(),
                key: lock_id.to_string(),
            });
        }

        Ok(())
    }

    fn vaults_release_lock_revealed_funds(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "vaults_unlock_revealed_funds";
        use crate::schema::vault_locks;

        diesel::delete(vault_locks::table)
            .filter(vault_locks::lock_id.eq(lock_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    // -------------------------------- Resource -------------------------------- //
    fn resources_upsert(
        &mut self,
        resource_address: &ResourceAddress,
        resource: &Resource,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "resources_insert";
        use crate::schema::resources;

        let resource_type = resource.resource_type().to_string();
        let total_supply = resource.total_supply().map(|a| a.to_string());
        let access_rules = serialize_json(resource.access_rules())?;
        let metadata = serialize_json(resource.metadata())?;
        let view_key = resource.view_key().map(serialize_hex);
        let divisibility = i32::from(resource.divisibility());
        let auth_hook = resource.auth_hook().map(serialize_json).transpose()?;
        let owner_rule = serialize_json(resource.owner_rule())?;

        diesel::insert_into(resources::table)
            .values((
                resources::address.eq(resource_address.to_string()),
                resources::resource_type.eq(&resource_type),
                resources::owner_rule.eq(&owner_rule),
                resources::token_symbol.eq(resource.token_symbol()),
                resources::divisibility.eq(divisibility),
                resources::access_rules.eq(&access_rules),
                resources::metadata.eq(&metadata),
                resources::view_key.eq(view_key.as_ref()),
                resources::total_supply.eq(total_supply.as_ref()),
                resources::auth_hook.eq(auth_hook.as_ref()),
            ))
            .on_conflict(resources::address)
            .do_update()
            .set((
                resources::resource_type.eq(&resource_type),
                resources::owner_rule.eq(&owner_rule),
                resources::token_symbol.eq(resource.token_symbol()),
                resources::divisibility.eq(divisibility),
                resources::access_rules.eq(&access_rules),
                resources::metadata.eq(&metadata),
                resources::view_key.eq(view_key.as_ref()),
                resources::total_supply.eq(total_supply.as_ref()),
                resources::auth_hook.eq(auth_hook.as_ref()),
                resources::updated_at.eq(diesel::dsl::now),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        Ok(())
    }

    // -------------------------------- Confidential Outputs -------------------------------- //

    fn confidential_outputs_lock_smallest_amount(
        &mut self,
        vault_id: &VaultId,
        lock_id: WalletLockId,
    ) -> Result<ConfidentialOutputModel, WalletStorageError> {
        use crate::schema::{accounts, confidential_outputs, vaults};

        let vault_db_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let locked_output = confidential_outputs::table
            .filter(confidential_outputs::vault_id.eq(vault_db_id))
            .filter(confidential_outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            // We have the key to spend
            .filter(confidential_outputs::owner_key_id.is_not_null())
            // `value` is u64 stored in a signed BigInt column; values >= 2^63 wrap to negative.
            // Sort non-negatives first (small u64s ascending) then negatives (large u64s ascending) to recover u64 order.
            .order_by(dsl::sql::<diesel::sql_types::Integer>(
                "CASE WHEN confidential_outputs.value < 0 THEN 1 ELSE 0 END",
            ))
            .then_order_by(confidential_outputs::value.asc())
            .first::<models::ConfidentialOutput>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let locked_output = locked_output.ok_or_else(|| WalletStorageError::NotFound {
            operation: "outputs_lock_smallest_amount",
            entity: "output".to_string(),
            key: format!("vault={}, lock_id={}", vault_id, lock_id),
        })?;

        let account_address = accounts::table
            .select(accounts::address)
            .filter(accounts::id.eq(locked_output.account_id))
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let changeset = (
            confidential_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()),
            confidential_outputs::lock_id.eq(lock_id),
            confidential_outputs::locked_at.eq(diesel::dsl::now),
        );
        diesel::update(confidential_outputs::table)
            .set(changeset)
            .filter(confidential_outputs::id.eq(locked_output.id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        Ok(ConfidentialOutputModel {
            account_address: ComponentAddress::from_str(&account_address).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "account address",
                    details: e.to_string(),
                }
            })?,
            vault_id: *vault_id,
            commitment: PedersenCommitmentBytes::from_hex(&locked_output.commitment).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "output commitment",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            value: (locked_output.value as u64).into(),
            sender_public_nonce: locked_output
                .sender_public_nonce
                .map(|nonce| {
                    RistrettoPublicKeyBytes::from_hex(&nonce).map_err(|e| WalletStorageError::DecodingError {
                        operation: "outputs_lock_smallest_amount",
                        item: "sender public nonce",
                        details: e.to_string(),
                    })
                })
                .transpose()?,
            view_only_key_id: deserialize_json(locked_output.view_only_key_id)?,
            owner_key_id: locked_output.owner_key_id.as_ref().map(deserialize_json).transpose()?,
            encrypted_data: EncryptedData::try_from(locked_output.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "encrypted data",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            public_asset_tag: None,
            memo: locked_output.memo_json.as_ref().map(deserialize_json).transpose()?,
            status: OutputStatus::LockedForSpend,
            lock_id: Some(lock_id),
        })
    }

    fn confidential_outputs_insert(&mut self, output: ConfidentialOutputModel) -> Result<(), WalletStorageError> {
        use crate::schema::{accounts, confidential_outputs, vaults};

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(&output.account_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        let vault_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(&output.vault_id.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        diesel::insert_into(confidential_outputs::table)
            .values((
                confidential_outputs::account_id.eq(account_id),
                confidential_outputs::vault_id.eq(vault_id),
                confidential_outputs::commitment.eq(output.commitment.to_hex()),
                // TODO: allow arbitrary precision in wallet
                confidential_outputs::value.eq(output.value.to_u64_checked().expect("value overflow u64") as i64),
                confidential_outputs::sender_public_nonce.eq(output.sender_public_nonce.map(|pk| pk.to_hex())),
                confidential_outputs::view_only_key_id.eq(serialize_json(&output.view_only_key_id)?),
                confidential_outputs::owner_key_id.eq(output.owner_key_id.as_ref().map(serialize_json).transpose()?),
                confidential_outputs::encrypted_data.eq(output.encrypted_data.as_ref()),
                confidential_outputs::memo_json.eq(output.memo.as_ref().map(serialize_json).transpose()?),
                confidential_outputs::status.eq(output.status.as_key_str()),
                confidential_outputs::lock_id.eq(output.lock_id),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        Ok(())
    }

    fn confidential_outputs_finalize_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        use crate::schema::confidential_outputs;

        // Unlock locked unconfirmed confidential_outputs
        diesel::update(confidential_outputs::table)
            .filter(confidential_outputs::lock_id.eq(lock_id))
            .filter(confidential_outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .set((
                confidential_outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                confidential_outputs::lock_id.eq::<Option<i32>>(None),
                confidential_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_finalize_by_proof_id", e))?;

        // Mark locked confidential_outputs as spent
        diesel::update(confidential_outputs::table)
            .filter(confidential_outputs::lock_id.eq(lock_id))
            .filter(confidential_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .set((
                confidential_outputs::status.eq(OutputStatus::Spent.as_key_str()),
                confidential_outputs::lock_id.eq::<Option<i32>>(None),
                confidential_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_finalize_by_proof_id", e))?;

        Ok(())
    }

    fn confidential_outputs_release_by_lock_id(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        use crate::schema::confidential_outputs;

        // Unlock locked unspent confidential_outputs
        diesel::update(confidential_outputs::table)
            .filter(confidential_outputs::lock_id.eq(lock_id))
            .filter(confidential_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .set((
                confidential_outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                confidential_outputs::lock_id.eq::<Option<i32>>(None),
                confidential_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_unlock_by_proof_id", e))?;

        // Remove confidential_outputs that were created by this lock
        diesel::delete(confidential_outputs::table)
            .filter(confidential_outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .filter(confidential_outputs::lock_id.eq(lock_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_unlock_by_proof_id", e))?;

        Ok(())
    }

    fn stealth_outputs_lock_smallest_amount(
        &mut self,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        lock_id: WalletLockId,
    ) -> Result<StealthOutputModel, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_lock_smallest_amount";
        use crate::schema::{accounts, stealth_outputs};

        self.ensure_lock_exists(lock_id)?;

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(account_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let locked_output = stealth_outputs::table
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(stealth_outputs::owner_account_id.eq(account_id))
            .filter(
                stealth_outputs::status
                    .eq(OutputStatus::Unspent.as_key_str())
                    // Allow locking a UTXO created within the transaction
                    .or(stealth_outputs::status
                        .eq(OutputStatus::LockedUnconfirmed.as_key_str())
                        .and(stealth_outputs::lock_id.eq(lock_id))),
            )
            // We have the key to spend
            .filter(stealth_outputs::owner_key_id.is_not_null())
            .filter(stealth_outputs::is_burnt.eq(false))
            .filter(stealth_outputs::is_frozen.eq(false))
            .filter(stealth_outputs::is_condition_spendable.eq(true))
            // `value` is u64 stored in a signed BigInt column; values >= 2^63 wrap to negative.
            // Sort non-negatives first (small u64s ascending) then negatives (large u64s ascending) to recover u64 order.
            .order_by(dsl::sql::<diesel::sql_types::Integer>(
                "CASE WHEN stealth_outputs.value < 0 THEN 1 ELSE 0 END",
            ))
            .then_order_by(stealth_outputs::value.asc())
            .first::<models::StealthOutput>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "stealth_output".to_string(),
                key: format!("lock_id={}, account_id={} ({})", lock_id, account_id, account_address),
            })?;

        let changeset = (
            stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()),
            stealth_outputs::lock_id.eq(lock_id),
            stealth_outputs::is_on_chain.eq(locked_output.status != OutputStatus::LockedUnconfirmed.as_key_str()),
            stealth_outputs::locked_at.eq(diesel::dsl::now),
        );
        diesel::update(stealth_outputs::table)
            .set(changeset)
            .filter(stealth_outputs::id.eq(locked_output.id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let mut output = locked_output.try_convert(*account_address)?;
        output.lock_id = Some(lock_id);
        Ok(output)
    }

    fn stealth_outputs_lock_many(
        &mut self,
        resource_address: &ResourceAddress,
        utxos: &[&PedersenCommitmentBytes],
        lock_id: WalletLockId,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_lock_many";
        use crate::schema::stealth_outputs;

        let num_rows = diesel::update(stealth_outputs::table)
            .set((
                stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()),
                stealth_outputs::lock_id.eq(lock_id),
                stealth_outputs::locked_at.eq(dsl::now),
            ))
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(stealth_outputs::commitment.eq_any(utxos.iter().map(|id| serialize_hex(id.as_ref()))))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_rows != utxos.len() {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "stealth_output".to_string(),
                key: format!(
                    "{}/{} found: resource_address={}, utxos={}",
                    num_rows,
                    utxos.len(),
                    resource_address,
                    utxos.display()
                ),
            });
        }

        Ok(())
    }

    fn stealth_outputs_insert(&mut self, output: &StealthOutputModel) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_insert";
        use crate::schema::{accounts, stealth_outputs};

        diesel::insert_into(stealth_outputs::table)
            .values((
                stealth_outputs::owner_account_id.eq(accounts::table
                    .select(accounts::id)
                    .filter(accounts::address.eq(output.owner_account.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
                stealth_outputs::resource_address.eq(output.resource_address.to_string()),
                stealth_outputs::commitment.eq(output.commitment.to_hex()),
                stealth_outputs::value.eq(output.value as i64),
                stealth_outputs::sender_public_nonce.eq(serialize_hex(output.sender_public_nonce)),
                stealth_outputs::view_only_key_id.eq(serialize_json(&output.view_only_key_id)?),
                stealth_outputs::owner_key_id.eq(output.owner_key_id.as_ref().map(serialize_json).transpose()?),
                stealth_outputs::encrypted_data.eq(output.encrypted_data.as_ref()),
                stealth_outputs::tag_byte.eq(output.tag_byte.value() as i32),
                stealth_outputs::memo_json.eq(output.memo.as_ref().map(serialize_json).transpose()?),
                stealth_outputs::spend_condition.eq(serialize_json(&output.spend_condition)?),
                stealth_outputs::minimum_value_promise.eq(output.minimum_value_promise as i64),
                stealth_outputs::is_on_chain.eq(output.is_on_chain),
                stealth_outputs::status.eq(output.status.as_key_str()),
                stealth_outputs::is_burnt.eq(output.is_burnt),
                stealth_outputs::is_frozen.eq(output.is_frozen),
                stealth_outputs::is_condition_spendable.eq(output.is_condition_spendable),
                stealth_outputs::lock_id.eq(output.lock_id),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn stealth_outputs_mark_as_spent(
        &mut self,
        resource_address: &ResourceAddress,
        id: &UtxoId,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_mark_as_spent";
        use crate::schema::stealth_outputs;

        let num_rows = diesel::update(stealth_outputs::table)
            .set((
                stealth_outputs::status.eq(OutputStatus::Spent.as_key_str()),
                stealth_outputs::lock_id.eq::<Option<i32>>(None),
                stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
                stealth_outputs::updated_at.eq(dsl::now),
            ))
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(stealth_outputs::commitment.eq(serialize_hex(id.into_commitment_bytes())))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "stealth_output".to_string(),
                key: format!("resource_address={}, id={}", resource_address, id),
            });
        }

        Ok(())
    }

    fn stealth_outputs_update(
        &mut self,
        address: &UtxoAddress,
        is_burnt: Option<bool>,
        status: Option<OutputStatus>,
        is_frozen: Option<bool>,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_update_status_from_utxo";
        use crate::schema::stealth_outputs;
        let update = StealthOutputUpdate {
            is_burnt,
            is_frozen,
            status: status.map(|s| s.as_key_str()),
            updated_at: dsl::now,
        };

        let num_rows = diesel::update(stealth_outputs::table)
            .set(update)
            .filter(stealth_outputs::resource_address.eq(address.resource_address().to_string()))
            .filter(stealth_outputs::commitment.eq(serialize_hex(address.id().into_commitment_bytes())))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "stealth_output".to_string(),
                key: format!("address={address}"),
            });
        }

        Ok(())
    }

    // locks
    fn locks_create(&mut self, timeout: Option<Duration>) -> Result<WalletLockId, WalletStorageError> {
        const OPERATION: &str = "locks_create";
        use crate::schema::locks;

        if let Some(timeout) = timeout {
            let timeout_seconds = i32::try_from(timeout.as_secs()).unwrap_or(i32::MAX);
            diesel::insert_into(locks::table)
                .values(locks::timeout_at.eq(dsl::sql(&format!("datetime('now', '+{} seconds')", timeout_seconds))))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        } else {
            diesel::insert_into(locks::table)
                .default_values()
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }
        // TODO: See if we can upgrade libSQLite 0.35
        let lock_id = locks::table
            .select(locks::id)
            .order_by(locks::id.desc())
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(lock_id as WalletLockId)
    }

    fn locks_delete(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "locks_delete";
        use crate::schema::locks;

        diesel::delete(locks::table.filter(locks::id.eq(lock_id)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn locks_link_transaction(
        &mut self,
        lock_id: WalletLockId,
        transaction_id: TransactionId,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "locks_link_transaction";
        use crate::schema::locks;

        diesel::update(locks::table.filter(locks::id.eq(lock_id)))
            .set((
                locks::transaction_id.eq(serialize_hex(transaction_id)),
                locks::timeout_at.eq(None::<PrimitiveDateTime>),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn locks_release_stale(&mut self) -> Result<usize, WalletStorageError> {
        const OPERATION: &str = "locks_release_stale";
        use crate::schema::locks;

        let stale_locks = locks::table
            .select(locks::id)
            .filter(locks::timeout_at.is_not_null())
            .filter(locks::timeout_at.le(dsl::now))
            .get_results::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        let num_stale = stale_locks.len();
        for lock_id in stale_locks {
            self.locks_release(lock_id)?;
        }

        Ok(num_stale)
    }

    fn locks_unlock_finalized(&mut self, lock_id: WalletLockId, diff: &SubstateDiff) -> Result<(), WalletStorageError> {
        self.stealth_outputs_finalize_by_lock_id(lock_id, diff)?;
        self.confidential_outputs_finalize_by_lock_id(lock_id)?;
        self.vaults_finalized_locked_revealed_funds(lock_id).optional()?;
        self.locks_delete(lock_id)?;
        Ok(())
    }

    fn locks_release(&mut self, lock_id: WalletLockId) -> Result<(), WalletStorageError> {
        self.confidential_outputs_release_by_lock_id(lock_id)?;
        self.stealth_outputs_release_by_lock_id(lock_id)?;
        self.vaults_release_lock_revealed_funds(lock_id)?;
        self.locks_delete(lock_id)?;

        Ok(())
    }

    // -------------------------------- Non fungible tokens -------------------------------- //
    fn non_fungible_token_upsert(&mut self, non_fungible_token: &NonFungibleToken) -> Result<(), WalletStorageError> {
        use crate::schema::{non_fungible_tokens, vaults};

        let data = serde_json::to_string(&non_fungible_token.data).map_err(|e| WalletStorageError::DecodingError {
            operation: "non_fungible_token_upsert",
            item: "non_fungible_tokens.data",
            details: e.to_string(),
        })?;

        let mutable_data =
            serde_json::to_string(&non_fungible_token.mutable_data).map_err(|e| WalletStorageError::DecodingError {
                operation: "non_fungible_token_upsert",
                item: "non_fungible_tokens.mutable_data",
                details: e.to_string(),
            })?;

        let vault_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(non_fungible_token.vault_id.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("proof_insert", e))?;

        diesel::insert_into(non_fungible_tokens::table)
            .values((
                non_fungible_tokens::nft_id.eq(non_fungible_token.nft_id.to_canonical_string()),
                non_fungible_tokens::data.eq(&data),
                non_fungible_tokens::resource_id.eq(non_fungible_token.resource_address.to_string()),
                non_fungible_tokens::mutable_data.eq(&mutable_data),
                non_fungible_tokens::vault_id.eq(vault_id),
                non_fungible_tokens::is_burnt.eq(non_fungible_token.is_burnt),
            ))
            .on_conflict((non_fungible_tokens::nft_id, non_fungible_tokens::vault_id))
            .do_update()
            .set((
                non_fungible_tokens::data.eq(&data),
                non_fungible_tokens::mutable_data.eq(&mutable_data),
                non_fungible_tokens::is_burnt.eq(non_fungible_token.is_burnt),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("non_fungible_token_upsert", e))?;

        info!(
            target: LOG_TARGET,
            "Inserted successfully new non fungible token with id = {}", non_fungible_token.nft_id
        );
        Ok(())
    }

    fn non_fungible_token_remove(
        &mut self,
        vault_id: &VaultId,
        non_fungible_id: &NonFungibleId,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::{non_fungible_tokens, vaults};

        let vault_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("non_fungible_token_remove", e))?;

        let num_affected = diesel::delete(non_fungible_tokens::table)
            .filter(non_fungible_tokens::nft_id.eq(non_fungible_id.to_canonical_string()))
            .filter(non_fungible_tokens::vault_id.eq(vault_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("non_fungible_token_remove", e))?;

        if num_affected == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "non_fungible_token_remove",
                entity: "non_fungible_token".to_string(),
                key: non_fungible_id.to_canonical_string(),
            });
        }

        Ok(())
    }

    fn webauthn_reg_insert(&mut self, username: String, passkey: Passkey) -> Result<(), WalletStorageError> {
        use crate::schema::{webauthn_registration_passkeys, webauthn_registrations};
        diesel::insert_into(webauthn_registrations::table)
            .values(webauthn_registrations::username.eq(username))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("webauthn_reg_insert", e))?;

        let registration_id: i32 =
            diesel::select(diesel::dsl::sql::<diesel::sql_types::Integer>("last_insert_rowid()"))
                .get_result(self.connection())
                .map_err(|e| WalletStorageError::general("webauthn_reg_insert", e))?;

        let passkey_json = serde_json::to_string(&passkey).map_err(|e| WalletStorageError::DecodingError {
            operation: "webauthn_reg_insert",
            item: "passkey",
            details: e.to_string(),
        })?;

        diesel::insert_into(webauthn_registration_passkeys::table)
            .values((
                webauthn_registration_passkeys::registration_id.eq(registration_id),
                webauthn_registration_passkeys::passkey.eq(passkey_json.as_bytes()),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("webauthn_reg_passkeys_insert", e))?;

        Ok(())
    }

    /// Inserting a new authored template.
    fn authored_templates_insert(&mut self, model: AuthoredTemplateModel) -> Result<(), WalletStorageError> {
        use crate::schema::authored_templates;

        diesel::insert_into(authored_templates::table)
            .values((
                authored_templates::author_public_key.eq(serialize_hex(model.author_public_key)),
                authored_templates::address.eq(serialize_hex(model.address)),
                authored_templates::name.eq(model.name),
                authored_templates::abi_version.eq(i32::from(model.abi_version)),
                authored_templates::functions.eq(serialize_json(&model.functions)?),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("authored_templates_insert", e))?;

        Ok(())
    }

    fn shard_state_version_set_many<I: IntoIterator<Item = (Shard, StateVersion)>>(
        &mut self,
        account_address: &ComponentAddress,
        resource_address: &ResourceAddress,
        state_versions: I,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "shard_state_version_set_many";
        use crate::schema::{accounts, resources, shard_state_versions};

        for (shard, state_version) in state_versions {
            diesel::insert_into(shard_state_versions::table)
                .values((
                    shard_state_versions::account_id.eq(accounts::table
                        .select(accounts::id)
                        .filter(accounts::address.eq(account_address.to_string()))
                        .limit(1)
                        .single_value()
                        .assume_not_null()),
                    shard_state_versions::resource_id.eq(resources::table
                        .select(resources::id)
                        .filter(resources::address.eq(resource_address.to_string()))
                        .limit(1)
                        .single_value()
                        .assume_not_null()),
                    shard_state_versions::shard.eq(shard.as_u32() as i32),
                    shard_state_versions::state_version.eq(state_version.as_u64() as i64),
                ))
                .on_conflict((
                    shard_state_versions::account_id,
                    shard_state_versions::resource_id,
                    shard_state_versions::shard,
                ))
                .do_update()
                .set(shard_state_versions::state_version.eq(state_version.as_u64() as i64))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }

        Ok(())
    }

    fn utxo_process_queue_extend<I: IntoIterator<Item = (ComponentAddress, UtxoUnspent)>>(
        &mut self,
        resource_address: &ResourceAddress,
        items: I,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "utxo_process_queue_extend";
        use crate::schema::{accounts, utxo_process_queue};

        for (account_address, unspent) in items {
            diesel::insert_into(utxo_process_queue::table)
                .values((
                    utxo_process_queue::account_id.eq(accounts::table
                        .select(accounts::id)
                        .filter(accounts::address.eq(account_address.to_string()))
                        .limit(1)
                        .single_value()
                        .assume_not_null()),
                    utxo_process_queue::utxo_tag.eq(unspent.tag.value() as i32),
                    utxo_process_queue::public_nonce.eq(serialize_hex(unspent.public_nonce)),
                    utxo_process_queue::resource_address.eq(resource_address.to_string()),
                ))
                .on_conflict_do_nothing()
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        }

        Ok(())
    }

    fn utxo_process_queue_remove_item(
        &mut self,
        resource_address: ResourceAddress,
        tag: UtxoTag,
        public_nonce: RistrettoPublicKeyBytes,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "utxo_process_queue_remove_item";
        use crate::schema::utxo_process_queue;

        let num_affected = diesel::delete(utxo_process_queue::table)
            .filter(utxo_process_queue::resource_address.eq(resource_address.to_string()))
            .filter(utxo_process_queue::utxo_tag.eq(tag.value() as i32))
            .filter(utxo_process_queue::public_nonce.eq(serialize_hex(public_nonce)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_affected == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "utxo_process_queue item".to_string(),
                key: format!(
                    "resource_address={}, tag={}, public_nonce={}",
                    resource_address, tag, public_nonce
                ),
            });
        }

        Ok(())
    }

    // Address book

    fn address_book_insert(
        &mut self,
        name: &str,
        address: &str,
        note: Option<&str>,
    ) -> Result<AddressBookEntry, WalletStorageError> {
        const OPERATION: &str = "address_book_insert";
        use crate::schema::address_book;

        diesel::insert_into(address_book::table)
            .values((
                address_book::name.eq(name),
                address_book::address.eq(address),
                address_book::note.eq(note),
            ))
            .execute(self.connection())
            .map_err(|e| map_address_book_error(OPERATION, name, e))?;

        let row = address_book::table
            .filter(address_book::name.eq(name))
            .first::<models::AddressBookEntry>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(AddressBookEntry {
            id: row.id,
            name: row.name,
            address: row.address,
            note: row.note,
        })
    }

    fn address_book_update(
        &mut self,
        name: &str,
        new_name: Option<&str>,
        address: Option<&str>,
        note: Option<&str>,
    ) -> Result<AddressBookEntry, WalletStorageError> {
        const OPERATION: &str = "address_book_update";
        use crate::schema::address_book;

        // Build a single changeset so all mutated columns (plus the
        // bookkeeping `updated_at`) are written in one UPDATE statement
        // instead of one query per field. The previous implementation issued
        // up to three separate UPDATEs, each with its own round-trip and its
        // own `updated_at` bump, leaving the row timestamps inconsistent with
        // the caller's intent.
        //
        // `note` is mapped `Some(s) -> Some(Some(s))` so the column is set to
        // the supplied string (including the empty string used by the UI to
        // clear a previously-stored note). `None` on any field means "leave
        // the column untouched".
        let changeset = AddressBookEntryChangeset {
            name: new_name,
            address,
            note: note.map(Some),
            updated_at: dsl::now,
        };

        let num_affected = diesel::update(address_book::table.filter(address_book::name.eq(name)))
            .set(changeset)
            .execute(self.connection())
            .map_err(|e| map_address_book_error(OPERATION, new_name.unwrap_or(name), e))?;

        if num_affected == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "address_book_entry".to_string(),
                key: name.to_string(),
            });
        }

        // After a successful rename the row now lives under `new_name`, so
        // the re-read must query by the post-update name.
        let lookup_name = new_name.unwrap_or(name);

        let row = address_book::table
            .filter(address_book::name.eq(lookup_name))
            .first::<models::AddressBookEntry>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(AddressBookEntry {
            id: row.id,
            name: row.name,
            address: row.address,
            note: row.note,
        })
    }

    fn address_book_delete(&mut self, name: &str) -> Result<(), WalletStorageError> {
        use crate::schema::address_book;

        let num_affected = diesel::delete(address_book::table.filter(address_book::name.eq(name)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("address_book_delete", e))?;

        if num_affected == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "address_book_delete",
                entity: "address_book_entry".to_string(),
                key: name.to_string(),
            });
        }

        Ok(())
    }

    fn api_key_insert(
        &mut self,
        name: &str,
        key_hash: &str,
        permissions: &str,
        expires_at: Option<time::PrimitiveDateTime>,
    ) -> Result<ApiKey, WalletStorageError> {
        const OPERATION: &str = "api_key_insert";
        use crate::schema::api_keys;

        diesel::insert_into(api_keys::table)
            .values((
                api_keys::name.eq(name),
                api_keys::key_hash.eq(key_hash),
                api_keys::permissions.eq(permissions),
                api_keys::expires_at.eq(expires_at),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let row = api_keys::table
            .filter(api_keys::key_hash.eq(key_hash))
            .first::<models::ApiKey>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(api_key_from_row(row))
    }

    fn api_key_touch_last_used(&mut self, id: i32, throttle: std::time::Duration) -> Result<(), WalletStorageError> {
        use crate::schema::api_keys;

        let now = time::OffsetDateTime::now_utc();
        // Use millisecond precision so sub-second throttle values aren't
        // silently zeroed by `as_secs` truncation.
        let cutoff = now - time::Duration::milliseconds(throttle.as_millis() as i64);
        let now = time::PrimitiveDateTime::new(now.date(), now.time());
        let cutoff = time::PrimitiveDateTime::new(cutoff.date(), cutoff.time());

        // Filter chain encodes three invariants:
        //   1. `revoked_at IS NULL` — never resurrect a revoked row. The auth shim verifies credentials and then spawns
        //      this in the background; a concurrent revoke must not be undone here.
        //   2. `last_used_at IS NULL OR last_used_at <= cutoff` — the throttle. Skips the write when the timestamp was
        //      bumped within the last `throttle` window, capping write QPS under a busy agent.
        //   3. `id = ?` — the obvious one.
        // `affected = 0` is the correct outcome for any of (1) revoked, (2)
        // throttled, or (3) unknown id — none of which the caller should
        // surface as an error.
        let _ = diesel::update(
            api_keys::table
                .filter(api_keys::id.eq(id))
                .filter(api_keys::revoked_at.is_null())
                .filter(
                    api_keys::last_used_at
                        .is_null()
                        .or(api_keys::last_used_at.le(Some(cutoff))),
                ),
        )
        .set(models::ApiKeyLastUsedChangeset {
            last_used_at: Some(now),
        })
        .execute(self.connection())
        .map_err(|e| WalletStorageError::general("api_key_touch_last_used", e))?;

        Ok(())
    }

    fn api_key_revoke(&mut self, id: i32) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "api_key_revoke";
        use crate::schema::api_keys;

        let now = time::OffsetDateTime::now_utc();
        let now = time::PrimitiveDateTime::new(now.date(), now.time());

        let num_affected = diesel::update(
            api_keys::table
                .filter(api_keys::id.eq(id))
                .filter(api_keys::revoked_at.is_null()),
        )
        .set(models::ApiKeyRevocationChangeset { revoked_at: Some(now) })
        .execute(self.connection())
        .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_affected == 0 {
            return Err(WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "api_key".to_string(),
                key: id.to_string(),
            });
        }

        Ok(())
    }
}

/// Convert a storage-layer `ApiKey` row into the SDK's `ApiKey` model. Kept
/// as a free function so it can be shared between the reader and writer
/// modules without re-implementing the field-by-field map.
fn api_key_from_row(row: models::ApiKey) -> ApiKey {
    ApiKey {
        id: row.id,
        name: row.name,
        key_hash: row.key_hash,
        permissions: row.permissions,
        created_at: row.created_at,
        last_used_at: row.last_used_at,
        revoked_at: row.revoked_at,
        expires_at: row.expires_at,
    }
}

/// Maps diesel errors from address_book writes into the typed
/// [`WalletStorageError::DuplicateName`] variant when the failure is a
/// SQLite `UNIQUE` constraint violation on the `address_book.name` column.
/// Every other error falls through to the generic path.
///
/// Matching on the typed [`DatabaseErrorKind::UniqueViolation`] keeps the
/// wallet SDK decoupled from the exact driver error text (e.g. sqlite's
/// `"UNIQUE constraint failed: address_book.name"`), so the UI can reliably
/// detect duplicate-name failures by matching on the `DuplicateName` token
/// rather than grepping the underlying driver message.
fn map_address_book_error(operation: &'static str, name: &str, err: diesel::result::Error) -> WalletStorageError {
    use diesel::result::{DatabaseErrorKind, Error as DieselError};
    match err {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
            WalletStorageError::DuplicateName { name: name.to_string() }
        },
        other => WalletStorageError::general(operation, other),
    }
}

impl WalletEventStoreWriter for WriteTransaction<'_> {
    fn append_wallet_event(&mut self, event: &WalletEvent) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "append_wallet_event";
        use crate::schema::{accounts, wallet_events};

        let (maybe_account, payload) = match event {
            WalletEvent::TransactionSubmitted(payload) => (
                payload
                    .context
                    .as_ref()
                    .and_then(|c| c.new_account_data())
                    .map(|a| a.address),
                serialize_json(payload)?,
            ),
            WalletEvent::TransactionFinalized(payload) => (None, serialize_json(payload)?),
            WalletEvent::TransactionInvalid(payload) => (None, serialize_json(payload)?),
            WalletEvent::AccountCreatedOnChain(payload) => {
                (Some(payload.account.component_address), serialize_json(payload)?)
            },
            WalletEvent::AccountChangedOnChain(payload) => (Some(payload.account_address), serialize_json(payload)?),
            WalletEvent::AuthLoginRequest(payload) => (None, serialize_json(payload)?),
            WalletEvent::UtxoRecoveryStarted(payload) => (None, serialize_json(payload)?),
            WalletEvent::UtxoRecovered(payload) => (Some(payload.account_address), serialize_json(payload)?),
            WalletEvent::UtxoRecoveryCompleted(payload) => (None, serialize_json(payload)?),
            WalletEvent::UtxoSpent(payload) => (Some(payload.account_address), serialize_json(payload)?),
        };

        let maybe_account = maybe_account.map(|addr| addr.to_string());

        let account_id = match maybe_account {
            Some(addr) => accounts::table
                .select(accounts::id)
                .filter(accounts::address.eq(addr))
                .first::<i32>(self.connection())
                .optional()
                .map_err(|e| WalletStorageError::general(OPERATION, e))?,
            None => None,
        };

        diesel::insert_into(wallet_events::table)
            .values((
                wallet_events::account_id.eq(account_id),
                wallet_events::event_type.eq(event.as_event_type().to_string()),
                wallet_events::event_data.eq(payload),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }
}

impl Drop for WriteTransaction<'_> {
    fn drop(&mut self) {
        if !self.transaction.is_done() {
            warn!(target: LOG_TARGET, "WriteTransaction was not committed or rolled back");
            if let Err(err) = self.transaction.rollback_internal() {
                warn!(target: LOG_TARGET, "Failed to rollback WriteTransaction: {}", err);
            }
        }
    }
}

impl<'a> Deref for WriteTransaction<'a> {
    type Target = ReadTransaction<'a>;

    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

impl DerefMut for WriteTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transaction
    }
}
