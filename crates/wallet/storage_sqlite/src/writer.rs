//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    iter,
    ops::{Add, Deref, DerefMut, Sub},
    str::FromStr,
    sync::MutexGuard,
};

use diesel::{NullableExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl, SqliteConnection};
use log::*;
use serde::Serialize;
use tari_bor::json_encoding::CborValueJsonSerializeWrapper;
use tari_engine_types::{resource::Resource, substate::SubstateId, UtxoAddress};
use tari_ootle_common_types::{shard::Shard, StateVersion, VersionedSubstateIdRef};
use tari_ootle_wallet_sdk::{
    models::{
        AccountUpdate,
        AuthoredTemplateModel,
        ConfidentialOutputModel,
        NewAccountData,
        NonFungibleToken,
        OutputLockId,
        OutputStatus,
        StealthOutputModel,
        SubstateModel,
        TransactionStatus,
        VaultModel,
        WalletTransactionUpdate,
    },
    storage::{WalletStorageError, WalletStoreReader, WalletStoreWriter},
};
use tari_template_lib::{
    models::{EncryptedData, NonFungibleId, ResourceAddress, VaultId},
    prelude::{ComponentAddress, PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    types::{Amount, TemplateAddress},
};
use tari_transaction::{Transaction, TransactionId};
use tari_utilities::hex::Hex;
use time::PrimitiveDateTime;
use webauthn_rs::prelude::Passkey;

use crate::{
    diesel::ExpressionMethods,
    helpers,
    models,
    models::StealthOutputUpdate,
    reader::ReadTransaction,
    schema::accounts,
    serialization::{serialize_hex, serialize_json},
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

    fn get_lock(&mut self, lock_id: OutputLockId) -> Result<models::OutputLock, WalletStorageError> {
        use crate::schema::output_locks;

        output_locks::table
            .filter(output_locks::id.eq(lock_id as i32))
            .first(self.connection())
            .map_err(|e| WalletStorageError::general("get_proof", e))
    }
}

impl WalletStoreWriter for WriteTransaction<'_> {
    fn commit(mut self) -> Result<(), WalletStorageError> {
        self.transaction.commit()?;
        Ok(())
    }

    fn rollback(mut self) -> Result<(), WalletStorageError> {
        self.transaction.rollback()?;
        Ok(())
    }

    fn jwt_add_empty_token(&mut self) -> Result<u64, WalletStorageError> {
        use crate::schema::auth_status;

        diesel::insert_into(auth_status::table)
            .values((auth_status::user_decided.eq(false), auth_status::granted.eq(false)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("jwt_add_empty_token", e))?;
        let last_inserted_id: i32 =
            diesel::select(diesel::dsl::sql::<diesel::sql_types::Integer>("last_insert_rowid()"))
                .get_result(self.connection())
                .map_err(|e| WalletStorageError::general("jwt_add_empty_token", e))?;
        Ok(last_inserted_id as u64)
    }

    fn jwt_store_decision(&mut self, id: u64, permissions_token: Option<&str>) -> Result<(), WalletStorageError> {
        use crate::schema::auth_status;
        // let values = match token {
        //     Some(token) => (auth_status::user_decided.eq(true),auth_status::granted.eq(true),auth_status::token)
        // }
        diesel::update(auth_status::table)
            .set((
                auth_status::user_decided.eq(true),
                auth_status::granted.eq(permissions_token.is_some()),
                permissions_token.map(|token| auth_status::token.eq(token)),
            ))
            .filter(auth_status::id.eq(id as i32))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("jwt_store_decision", e))?;
        Ok(())
    }

    fn jwt_is_revoked(&mut self, token: &str) -> Result<bool, WalletStorageError> {
        use crate::schema::auth_status;
        let revoked = auth_status::table
            .select(auth_status::revoked)
            .filter(auth_status::token.eq(token))
            .first(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("jwt_is_revoked", e))?;
        match revoked {
            Some(revoked) => Ok(revoked),
            None => {
                // We don't know this token. Store it as not revoked. Weirdly if the token is used with different daemon
                // it will work even if it's revoked in this one. But since the user will need to confirm any actions
                // there should be no security issue.
                diesel::insert_into(auth_status::table)
                    .values((
                        auth_status::granted.eq(true),
                        auth_status::user_decided.eq(true),
                        auth_status::token.eq(token),
                    ))
                    .execute(self.connection())
                    .map_err(|e| WalletStorageError::general("jwt_is_revoked", e))?;
                Ok(false)
            },
        }
    }

    fn jwt_revoke(&mut self, token_id: i32) -> Result<(), WalletStorageError> {
        use crate::schema::auth_status;
        if diesel::update(auth_status::table)
            .set(auth_status::revoked.eq(true))
            .filter(auth_status::id.eq(token_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("jwt_revoke", e))? ==
            0
        {
            diesel::insert_into(auth_status::table)
                .values((auth_status::revoked.eq(true), auth_status::id.eq(token_id)))
                .execute(self.connection())
                .map_err(|e| WalletStorageError::general("jwt_revoke", e))?;
        }
        Ok(())
    }

    // -------------------------------- KeyManager -------------------------------- //

    fn key_manager_insert(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
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
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_insert", e))?;

        Ok(())
    }

    fn key_manager_set_active_index(&mut self, branch: &str, index: u64) -> Result<(), WalletStorageError> {
        use crate::schema::key_manager_states;
        let index = i64::try_from(index)
            .map_err(|_| WalletStorageError::general("key_manager_set_active_index", "index too large"))?;

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
        is_dry_run: bool,
    ) -> Result<(), WalletStorageError> {
        use crate::schema::transactions;

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
                transactions::transaction_id.eq(serialize_hex(transaction.calculate_id())),
                transactions::transaction_json.eq(serialize_json(transaction)?),
                transactions::referenced_components.eq(serialize_json(&ref_components)?),
                transactions::signers.eq(serialize_json(&signers)?),
                transactions::status.eq(TransactionStatus::New.as_key_str()),
                transactions::new_account_info.eq(new_account_info.map(serialize_json).transpose()?),
                transactions::dry_run.eq(is_dry_run),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("transactions_insert", e))?;

        Ok(())
    }

    fn transactions_update(&mut self, update: WalletTransactionUpdate<'_>) -> Result<(), WalletStorageError> {
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
            .map_err(|e| WalletStorageError::general("transactions_set_result_and_status", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "transactions_set_result_and_status",
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
        owner_key_index: u64,
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
                accounts::owner_key_index.eq(owner_key_index as i64),
                accounts::is_confirmed_on_chain.eq(is_confirmed_on_chain),
                accounts::is_default.eq(is_default),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_insert", e))?;

        Ok(())
    }

    fn accounts_update(&mut self, address: &ComponentAddress, update: AccountUpdate) -> Result<(), WalletStorageError> {
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
            return Err(WalletStorageError::NotFound {
                operation: "accounts_update",
                entity: "account".to_string(),
                key: address.to_string(),
            });
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
            // TODO: consider migrating to a string
            vaults::revealed_balance.eq(vault
                .revealed_balance
                .to_u64_checked()
                .expect("revealed balance is too large") as i64),
            vaults::confidential_balance.eq(vault
                .confidential_balance
                .to_u64_checked()
                .expect("confidential balance is too large") as i64),
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
            vaults::revealed_balance.eq(revealed_balance
                .to_u64_checked()
                .expect("revealed balance is too large") as i64),
            vaults::confidential_balance.eq(confidential_balance
                .to_u64_checked()
                .expect("revealed balance is too large") as i64),
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

    fn vaults_lock_revealed_funds(
        &mut self,
        lock_id: OutputLockId,
        amount_to_lock: Amount,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "vaults_lock_revealed_funds";
        use crate::schema::{output_locks, vaults};

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

        // TODO: we add using sql, limiting the max to i64::MAX. Amounts should/could be represented as a string, output
        // amount is limited by bulletproofs to u64::MAX.
        let amount_to_lock = amount_to_lock
            .to_u64_checked()
            .ok_or_else(|| WalletStorageError::bad_query(OPERATION, "amount to lock is too large"))?;
        let amount_to_lock = i64::try_from(amount_to_lock).map_err(|_| {
            WalletStorageError::bad_query(OPERATION, "amount to lock is too large, must be less than i64::MAX")
        })?;

        let changeset =
            output_locks::locked_revealed_amount.eq(output_locks::locked_revealed_amount.add(amount_to_lock));

        let num_rows = diesel::update(output_locks::table)
            .set(changeset)
            .filter(output_locks::id.eq(lock_id as i32))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "vaults_lock_revealed_funds",
                entity: "lock".to_string(),
                key: lock_id.to_string(),
            });
        }

        let lock = self.get_lock(lock_id)?;
        let vault_id = lock.vault_id.ok_or_else(|| WalletStorageError::BadQuery {
            operation: "vaults_lock_revealed_funds",
            details: format!("lock {} does not lock a vault", lock_id),
        })?;

        let changeset = vaults::locked_revealed_balance.eq(vaults::locked_revealed_balance.add(amount_to_lock));

        let num_rows = diesel::update(vaults::table)
            .set(changeset)
            .filter(vaults::id.eq(vault_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_lock_revealed_funds", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "vaults_lock_revealed_funds",
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            });
        }

        Ok(())
    }

    fn vaults_finalized_locked_revealed_funds(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        use crate::schema::vaults;

        let lock = self.get_lock(lock_id)?;

        let Some(vault_id) = lock.vault_id else {
            debug!(
                target: LOG_TARGET,
                "Lock {} does not lock a vault, skipping vaults_finalized_locked_revealed_funds",
                lock_id
            );
            // Lock does not lock a vault, therefore, does not lock revealed function = No-op
            return Ok(());
        };

        let changeset = (
            vaults::revealed_balance.eq(vaults::revealed_balance.sub(lock.locked_revealed_amount)),
            vaults::locked_revealed_balance.eq(vaults::locked_revealed_balance.sub(lock.locked_revealed_amount)),
        );

        let num_rows = diesel::update(vaults::table)
            .set(changeset)
            .filter(vaults::id.eq(vault_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_finalized_locked_funds", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "vaults_finalized_locked_funds",
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            });
        }

        Ok(())
    }

    fn vaults_unlock_revealed_funds(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        use crate::schema::vaults;

        let lock = self.get_lock(lock_id)?;
        let Some(vault_id) = lock.vault_id else {
            // Lock does not lock a vault, therefore, does not lock revealed function = No-op
            return Ok(());
        };

        let changeset =
            vaults::locked_revealed_balance.eq(vaults::locked_revealed_balance.sub(lock.locked_revealed_amount));

        let num_rows = diesel::update(vaults::table)
            .set(changeset)
            .filter(vaults::id.eq(vault_id))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_unlock_revealed_funds", e))?;

        if num_rows == 0 {
            return Err(WalletStorageError::NotFound {
                operation: "vaults_unlock_revealed_funds",
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            });
        }

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
        let owner_key = resource.owner_key().map(serialize_hex);
        let owner_rule = serialize_json(resource.owner_rule())?;

        diesel::insert_into(resources::table)
            .values((
                resources::address.eq(resource_address.to_string()),
                resources::resource_type.eq(&resource_type),
                resources::owner_key.eq(owner_key.as_ref()),
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
                resources::owner_key.eq(owner_key.as_ref()),
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

    // -------------------------------- Outputs -------------------------------- //

    fn outputs_lock_smallest_amount(
        &mut self,
        vault_id: &VaultId,
        lock_id: OutputLockId,
    ) -> Result<ConfidentialOutputModel, WalletStorageError> {
        use crate::schema::{accounts, outputs, vaults};

        let vault_db_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_lock_smallest_amount", e))?;

        let locked_output = outputs::table
            .filter(outputs::vault_id.eq(vault_db_id))
            .filter(outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            .order_by(outputs::value.asc())
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
            outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()),
            outputs::lock_id.eq(lock_id as i32),
            outputs::locked_at.eq(diesel::dsl::now),
        );
        diesel::update(outputs::table)
            .set(changeset)
            .filter(outputs::id.eq(locked_output.id))
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
            encryption_secret_key_index: locked_output.encryption_secret_key_index as u64,
            encrypted_data: EncryptedData::try_from(locked_output.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "encrypted data",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            public_asset_tag: None,
            status: OutputStatus::LockedForSpend,
            lock_id: Some(lock_id),
        })
    }

    fn outputs_insert(&mut self, output: ConfidentialOutputModel) -> Result<(), WalletStorageError> {
        use crate::schema::{accounts, outputs, vaults};

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

        diesel::insert_into(outputs::table)
            .values((
                outputs::account_id.eq(account_id),
                outputs::vault_id.eq(vault_id),
                outputs::commitment.eq(output.commitment.to_hex()),
                // TODO: allow arbitrary precision in wallet
                outputs::value.eq(output.value.to_u64_checked().expect("value overflow u64") as i64),
                outputs::sender_public_nonce.eq(output.sender_public_nonce.map(|pk| pk.to_hex())),
                outputs::encryption_secret_key_index.eq(output.encryption_secret_key_index as i64),
                outputs::encrypted_data.eq(output.encrypted_data.as_ref()),
                outputs::status.eq(output.status.as_key_str()),
                outputs::lock_id.eq(output.lock_id.map(|v| v as i32)),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_insert", e))?;

        Ok(())
    }

    fn outputs_finalize_by_lock_id(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        use crate::schema::outputs;

        // Unlock locked unconfirmed outputs
        diesel::update(outputs::table)
            .filter(outputs::lock_id.eq(lock_id as i32))
            .filter(outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .set((
                outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                outputs::lock_id.eq::<Option<i32>>(None),
                outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_finalize_by_proof_id", e))?;

        // Mark locked outputs as spent
        diesel::update(outputs::table)
            .filter(outputs::lock_id.eq(lock_id as i32))
            .filter(outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .set((
                outputs::status.eq(OutputStatus::Spent.as_key_str()),
                outputs::lock_id.eq::<Option<i32>>(None),
                outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_finalize_by_proof_id", e))?;

        Ok(())
    }

    fn outputs_release_by_lock_id(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        use crate::schema::outputs;

        // Unlock locked unspent outputs
        diesel::update(outputs::table)
            .filter(outputs::lock_id.eq(lock_id as i32))
            .filter(outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .set((
                outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                outputs::lock_id.eq::<Option<i32>>(None),
                outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_unlock_by_proof_id", e))?;

        // Remove outputs that were created by this lock
        diesel::delete(outputs::table)
            .filter(outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .filter(outputs::lock_id.eq(lock_id as i32))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_unlock_by_proof_id", e))?;

        Ok(())
    }

    fn stealth_outputs_lock_smallest_amount(
        &mut self,
        account_address: &ComponentAddress,
        lock_id: OutputLockId,
    ) -> Result<StealthOutputModel, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_lock_smallest_amount";
        use crate::schema::stealth_outputs;

        let lock = self.get_lock(lock_id)?;

        let account_id = accounts::table
            .select(accounts::id)
            .filter(accounts::address.eq(account_address.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let locked_output = stealth_outputs::table
            .filter(stealth_outputs::resource_address.eq(&lock.resource_address))
            .filter(stealth_outputs::owner_account_id.eq(account_id))
            .filter(stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            .filter(stealth_outputs::is_burnt.eq(false))
            .filter(stealth_outputs::is_frozen.eq(false))
            .order_by(stealth_outputs::value.asc())
            .first::<models::StealthOutput>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "stealth_output".to_string(),
                key: format!(
                    "resource={}, lock_id={}, account_id={} ({})",
                    lock.resource_address, lock_id, account_id, account_address
                ),
            })?;

        let changeset = (
            stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()),
            stealth_outputs::lock_id.eq(lock_id as i32),
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
                stealth_outputs::value.eq(output.value.to_string()),
                stealth_outputs::sender_public_nonce.eq(serialize_hex(output.sender_public_nonce)),
                stealth_outputs::encryption_secret_key_index.eq(output.encryption_secret_key_index as i64),
                stealth_outputs::encrypted_data.eq(output.encrypted_data.as_ref()),
                stealth_outputs::tag_byte.eq(i32::from(output.tag_byte.as_byte())),
                stealth_outputs::status.eq(output.status.as_key_str()),
                stealth_outputs::is_burnt.eq(output.is_burnt),
                stealth_outputs::is_frozen.eq(output.is_frozen),
                stealth_outputs::lock_id.eq(output.lock_id.map(|v| v as i32)),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn stealth_outputs_mark_as_spent(&mut self, address: &UtxoAddress) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_mark_as_spent";
        use crate::schema::stealth_outputs;

        let num_rows = diesel::update(stealth_outputs::table)
            .set((
                stealth_outputs::status.eq(OutputStatus::Spent.as_key_str()),
                stealth_outputs::lock_id.eq::<Option<i32>>(None),
                stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
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

    fn stealth_outputs_finalize_by_lock_id(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_finalize_by_lock_id";
        use crate::schema::stealth_outputs;

        // Unlock locked unconfirmed stealth_outputs
        diesel::update(stealth_outputs::table)
            .filter(stealth_outputs::lock_id.eq(lock_id as i32))
            .filter(stealth_outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .set((
                stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                stealth_outputs::lock_id.eq::<Option<i32>>(None),
                stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        // Mark locked outputs as spent
        diesel::update(stealth_outputs::table)
            .filter(stealth_outputs::lock_id.eq(lock_id as i32))
            .filter(stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .set((
                stealth_outputs::status.eq(OutputStatus::Spent.as_key_str()),
                stealth_outputs::lock_id.eq::<Option<i32>>(None),
                stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    fn stealth_outputs_release_by_lock_id(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_release_by_lock_id";
        use crate::schema::stealth_outputs;

        // Unlock locked unspent stealth_outputs
        diesel::update(stealth_outputs::table)
            .filter(stealth_outputs::lock_id.eq(lock_id as i32))
            .filter(stealth_outputs::status.eq(OutputStatus::LockedForSpend.as_key_str()))
            .set((
                stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()),
                stealth_outputs::lock_id.eq::<Option<i32>>(None),
                stealth_outputs::locked_at.eq::<Option<PrimitiveDateTime>>(None),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        // Remove stealth_outputs that were created by this lock
        diesel::delete(stealth_outputs::table)
            .filter(stealth_outputs::status.eq(OutputStatus::LockedUnconfirmed.as_key_str()))
            .filter(stealth_outputs::lock_id.eq(lock_id as i32))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

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
            updated_at: Some(helpers::now()),
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

    // Output locks
    fn output_locks_insert(&mut self, resource_address: &ResourceAddress) -> Result<OutputLockId, WalletStorageError> {
        const OPERATION: &str = "stealth_locks_insert";
        use crate::schema::output_locks;

        diesel::insert_into(output_locks::table)
            .values(output_locks::resource_address.eq(resource_address.to_string()))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;
        // TODO: See if we can upgrade libSQLite 0.35
        let lock_id = output_locks::table
            .select(output_locks::id)
            .order_by(output_locks::id.desc())
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(lock_id as OutputLockId)
    }

    fn output_locks_insert_for_vault(&mut self, vault_id: &VaultId) -> Result<OutputLockId, WalletStorageError> {
        const OPERATION: &str = "output_locks_insert";
        use crate::schema::{output_locks, vaults};

        let (vault_id, resource_address) = vaults::table
            .select((vaults::id, vaults::resource_address))
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<(i32, String)>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        diesel::insert_into(output_locks::table)
            .values((
                output_locks::resource_address.eq(resource_address),
                output_locks::vault_id.eq(vault_id),
            ))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        // RETURNING only available from SQLite 3.35 https://www.sqlite.org/lang_returning.html
        // TODO: See if we can upgrade SQLite
        let lock_id = output_locks::table
            .select(output_locks::id)
            .order_by(output_locks::id.desc())
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(lock_id as OutputLockId)
    }

    fn output_locks_delete(&mut self, lock_id: OutputLockId) -> Result<(), WalletStorageError> {
        use crate::schema::output_locks;

        diesel::delete(output_locks::table.filter(output_locks::id.eq(lock_id as i32)))
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("output_locks_delete", e))?;

        Ok(())
    }

    fn output_locks_set_params(
        &mut self,
        lock_id: OutputLockId,
        transaction_id: Option<TransactionId>,
        vault_id: Option<VaultId>,
    ) -> Result<(), WalletStorageError> {
        const OPERATION: &str = "output_locks_set_params";
        use crate::schema::output_locks;

        if transaction_id.is_none() && vault_id.is_none() {
            return Err(WalletStorageError::BadQuery {
                operation: "output_locks_set_params",
                details: "At least one of transaction_id or vault_id must be provided".to_string(),
            });
        }

        #[derive(AsChangeset)]
        #[diesel(table_name = output_locks)]
        struct UpdateOutputLock {
            vault_id: Option<Option<i32>>,
            transaction_hash: Option<String>,
        }

        let vault_db_id = if let Some(vault_id) = vault_id {
            use crate::schema::vaults;

            vaults::table
                .select(vaults::id)
                .filter(vaults::address.eq(vault_id.to_string()))
                .first::<i32>(self.connection())
                .map(Some)
                .map(Some)
                .map_err(|e| WalletStorageError::general(OPERATION, e))?
        } else {
            None
        };

        let update_set = UpdateOutputLock {
            vault_id: vault_db_id,
            transaction_hash: transaction_id.map(|t| t.to_string()),
        };

        diesel::update(output_locks::table.filter(output_locks::id.eq(lock_id as i32)))
            .set(update_set)
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(())
    }

    // -------------------------------- Non fungible tokens -------------------------------- //
    fn non_fungible_token_upsert(&mut self, non_fungible_token: &NonFungibleToken) -> Result<(), WalletStorageError> {
        use crate::schema::{non_fungible_tokens, vaults};

        let data = serde_json::to_string(&CborValueJsonSerializeWrapper(&non_fungible_token.data)).map_err(|e| {
            WalletStorageError::DecodingError {
                operation: "non_fungible_token_upsert",
                item: "non_fungible_tokens.data",
                details: e.to_string(),
            }
        })?;

        let mutable_data = serde_json::to_string(&CborValueJsonSerializeWrapper(&non_fungible_token.mutable_data))
            .map_err(|e| WalletStorageError::DecodingError {
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
                authored_templates::tari_version.eq(model.tari_version),
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
}

impl Drop for WriteTransaction<'_> {
    fn drop(&mut self) {
        if !self.transaction.is_done() {
            warn!(target: LOG_TARGET, "WriteTransaction was not committed or rolled back");
            if let Err(err) = self.transaction.rollback() {
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
