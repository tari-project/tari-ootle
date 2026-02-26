//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::MutexGuard,
};

use bigdecimal::{BigDecimal, ToPrimitive};
use diesel::{
    BoolExpressionMethods,
    ExpressionMethods,
    JoinOnDsl,
    NullableExpressionMethods,
    OptionalExtension,
    QueryDsl,
    RunQueryDsl,
    SelectableHelper,
    SqliteConnection,
    TextExpressionMethods,
    dsl,
    dsl::sum,
    sql_query,
    sql_types,
};
use log::{error, warn};
use serde::de::DeserializeOwned;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{StateVersion, shard::Shard, substate_type::SubstateType};
use tari_ootle_transaction::TransactionId;
use tari_ootle_wallet_sdk::{
    models::{
        Account,
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
        WebauthnRegistrationPasskeyModel,
    },
    storage::{TagAndPublicNoncePair, WalletStorageError, WalletStoreReader},
};
use tari_template_lib_types::{
    Amount,
    ComponentAddress,
    NonFungibleId,
    ResourceAddress,
    ResourceType,
    TemplateAddress,
    VaultId,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
};
use webauthn_rs::prelude::Passkey;

use crate::{
    models,
    models::{AuthoredTemplate, WebauthnRegistrationPasskey},
    schema::accounts,
    serialization::{deserialize_hex_try_from, deserialize_json, serialize_hex},
};

const LOG_TARGET: &str = "tari::ootle::wallet_sdk::storage_sqlite::reader";

pub struct ReadTransaction<'a> {
    connection: MutexGuard<'a, SqliteConnection>,
    is_done: bool,
}

impl<'a> ReadTransaction<'a> {
    pub fn new(connection: MutexGuard<'a, SqliteConnection>) -> Self {
        Self {
            connection,
            is_done: false,
        }
    }

    pub(super) fn is_done(&self) -> bool {
        self.is_done
    }

    pub(super) fn connection(&mut self) -> &mut SqliteConnection {
        &mut self.connection
    }

    /// Internal commit
    pub(super) fn commit_internal(&mut self) -> Result<(), WalletStorageError> {
        sql_query("COMMIT")
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("commit", e))?;
        self.is_done = true;
        Ok(())
    }

    /// Internal rollback
    pub(super) fn rollback_internal(&mut self) -> Result<(), WalletStorageError> {
        sql_query("ROLLBACK")
            .execute(self.connection())
            .map_err(|e| WalletStorageError::general("rollback", e))?;
        self.is_done = true;
        Ok(())
    }
}

impl WalletStoreReader for ReadTransaction<'_> {
    // -------------------------------- KeyManager -------------------------------- //

    fn key_manager_get_all(&mut self, branch: &str) -> Result<Vec<(u64, bool)>, WalletStorageError> {
        use crate::schema::key_manager_states;

        let results = key_manager_states::table
            .select((key_manager_states::index, key_manager_states::is_active))
            .filter(key_manager_states::branch_seed.eq(branch))
            .get_results::<(i64, bool)>(self.connection())
            .map_err(|e| WalletStorageError::general("key_manager_get_all", e))?;

        Ok(results
            .into_iter()
            .map(|(index, is_active)| (index as u64, is_active))
            .collect())
    }

    fn key_manager_get_active_index(&mut self, branch: &str) -> Result<u64, WalletStorageError> {
        use crate::schema::key_manager_states;

        key_manager_states::table
            .select(key_manager_states::index)
            .filter(key_manager_states::branch_seed.eq(branch))
            .filter(key_manager_states::is_active.eq(true))
            .first(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("key_manager_get_active_index", e))?
            .map(|index: i64| index as u64)
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "key_manager_get_active_index",
                entity: "key_manager_state".to_string(),
                key: branch.to_string(),
            })
    }

    fn key_manager_get_last_index(&mut self, branch: &str) -> Result<u64, WalletStorageError> {
        use crate::schema::key_manager_states;

        key_manager_states::table
            .select(key_manager_states::index)
            .filter(key_manager_states::branch_seed.eq(branch))
            .order(key_manager_states::index.desc())
            .first(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("key_manager_get_last_index", e))?
            .map(|index: i64| index as u64)
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "key_manager_get_last_index",
                entity: "key_manager_state".to_string(),
                key: branch.to_string(),
            })
    }

    fn key_manager_get_raw_imported_key(&mut self, id: u64) -> Result<(KeyType, Box<[u8]>), WalletStorageError> {
        const OPERATION: &str = "key_manager_get_raw_imported_key";
        use crate::schema::key_manager_imported_keys;

        let (key_type, data) = key_manager_imported_keys::table
            .select((
                key_manager_imported_keys::key_type,
                key_manager_imported_keys::encrypted_secret,
            ))
            .filter(key_manager_imported_keys::id.eq(id as i32))
            .first::<(String, Vec<u8>)>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "imported_key".to_string(),
                key: id.to_string(),
            })?;

        Ok((
            key_type
                .parse::<KeyType>()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: OPERATION,
                    item: "imported_key",
                    details: format!("Failed to parse key type: {}", key_type),
                })?,
            data.into_boxed_slice(),
        ))
    }

    // -------------------------------- Config -------------------------------- //
    fn config_get<T: DeserializeOwned>(&mut self, key: &str) -> Result<Config<T>, WalletStorageError> {
        let config = self.config_get_string(key)?;

        Ok(Config {
            key: config.key,
            value: deserialize_json(&config.value)?,
            is_encrypted: config.is_encrypted,
            created_at: config.created_at,
            updated_at: config.updated_at,
        })
    }

    fn config_get_string(&mut self, key: &str) -> Result<Config<String>, WalletStorageError> {
        use crate::schema::config;

        let config = config::table
            .filter(config::key.eq(key))
            .first::<models::Config>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("config_get", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "config_get",
                entity: "config".to_string(),
                key: key.to_string(),
            })?;

        Ok(Config {
            key: config.key,
            value: config.value,
            is_encrypted: config.is_encrypted,
            created_at: config.created_at,
            updated_at: config.updated_at,
        })
    }

    fn config_exists(&mut self, key: &str) -> Result<bool, WalletStorageError> {
        use crate::schema::config;

        let exists = config::table
            .filter(config::key.eq(key))
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("config_exists", e))?;

        Ok(exists > 0)
    }

    // -------------------------------- Transactions -------------------------------- //
    fn transactions_get(&mut self, transaction_id: TransactionId) -> Result<WalletTransaction, WalletStorageError> {
        use crate::schema::transactions;
        let row = transactions::table
            .filter(transactions::transaction_id.eq(serialize_hex(transaction_id)))
            .first::<models::TransactionRecord>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("transaction_get", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "transaction_get",
                entity: "transaction".to_string(),
                key: transaction_id.to_string(),
            })?;

        let transaction = row.try_into_wallet_transaction()?;
        Ok(transaction)
    }

    fn transactions_fetch_all(
        &mut self,
        status: Option<TransactionStatus>,
        component: Option<ComponentAddress>,
        signed_by_public_key: Option<RistrettoPublicKeyBytes>,
    ) -> Result<Vec<WalletTransaction>, WalletStorageError> {
        use crate::schema::transactions;

        let mut query = transactions::table.into_boxed().filter(transactions::dry_run.eq(false));
        if let Some(status) = status {
            query = query.filter(transactions::status.eq(status.as_key_str()));
        }
        if let Some(component) = component {
            if let Some(public_key) = signed_by_public_key {
                query = query.filter(
                    transactions::referenced_components
                        .like(format!("%{}%", component))
                        .or(transactions::signers.like(format!("%{}%", serialize_hex(public_key)))),
                );
            } else {
                query = query.filter(transactions::referenced_components.like(format!("%{}%", component)));
            }
        } else if let Some(public_key) = signed_by_public_key {
            query = query.filter(transactions::signers.like(format!("%{}%", serialize_hex(public_key))));
        } else {
            // No filter
        }
        let rows = query
            .order(transactions::created_at.desc())
            .load::<models::TransactionRecord>(self.connection())
            .map_err(|e| WalletStorageError::general("transactions_fetch_all", e))?;

        rows.into_iter().map(|row| row.try_into_wallet_transaction()).collect()
    }

    // -------------------------------- Substates -------------------------------- //
    fn substates_get(&mut self, address: &SubstateId) -> Result<SubstateModel, WalletStorageError> {
        const OPERATION: &str = "substates_get";
        use crate::schema::substates;

        let rec = substates::table
            .filter(substates::address.eq(address.to_string()))
            .first::<models::Substate>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "substate".to_string(),
                key: address.to_string(),
            })?;

        let rec = rec.try_to_record()?;
        Ok(rec)
    }

    fn substates_get_all(
        &mut self,
        by_type: Option<SubstateType>,
        by_template_address: Option<&TemplateAddress>,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<SubstateModel>, WalletStorageError> {
        use crate::schema::substates;

        let mut query = substates::table.into_boxed();
        if let Some(template_address) = by_template_address {
            query = query.filter(substates::template_address.eq(template_address.to_string()));
        }
        if let Some(substate_type) = by_type {
            match substate_type {
                SubstateType::NonFungible => {
                    query = query
                        .filter(substates::address.like(format!("resource_% {}_%", substate_type.as_prefix_str())));
                },
                _ => {
                    query = query.filter(substates::address.like(format!("{}_%", substate_type.as_prefix_str())));
                },
            }
        }
        if let Some(limit) = limit {
            query = query.limit(limit as i64);
        }
        if let Some(offset) = offset {
            query = query.offset(offset as i64);
        }

        let rows = query
            .get_results::<models::Substate>(self.connection())
            .map_err(|e| WalletStorageError::general("substates_get_all", e))?;

        rows.into_iter().map(|rec| rec.try_to_record()).collect()
    }

    fn substates_get_children(&mut self, parent: &SubstateId) -> Result<Vec<SubstateModel>, WalletStorageError> {
        use crate::schema::substates;

        let rows = substates::table
            .filter(substates::parent_address.eq(parent.to_string()))
            .get_results::<models::Substate>(self.connection())
            .map_err(|e| WalletStorageError::general("substates_get_children", e))?;

        rows.into_iter().map(|rec| rec.try_to_record()).collect()
    }

    // -------------------------------- Accounts -------------------------------- //
    fn accounts_get(&mut self, address: &ComponentAddress) -> Result<Account, WalletStorageError> {
        use crate::schema::accounts;

        let row = accounts::table
            .filter(accounts::address.eq(address.to_string()))
            .first::<models::Account>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("accounts_get", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "accounts_get",
                entity: "account".to_string(),
                key: address.to_string(),
            })?;

        let account = row.try_convert().map_err(|e| WalletStorageError::DecodingError {
            operation: "accounts_get",
            item: "account",
            details: format!("Failed to convert SQL record to Account: {}", e),
        })?;
        Ok(account)
    }

    fn accounts_get_many(&mut self, offset: usize, limit: usize) -> Result<Vec<Account>, WalletStorageError> {
        use crate::schema::accounts;

        let rows = accounts::table
            .limit(limit as i64)
            .offset(offset as i64)
            .get_results::<models::Account>(self.connection())
            .map_err(|e| WalletStorageError::general("accounts_get_many", e))?;

        let accs = rows
            .into_iter()
            .map(|row| row.try_convert())
            .collect::<Result<_, _>>()?;
        Ok(accs)
    }

    fn accounts_get_default(&mut self) -> Result<Account, WalletStorageError> {
        use crate::schema::accounts;

        let row = accounts::table
            .filter(accounts::is_default.eq(true))
            .first::<models::Account>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("accounts_get_default", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "accounts_get_default",
                entity: "account".to_string(),
                key: "default".to_string(),
            })?;

        row.try_convert()
    }

    fn accounts_count(&mut self) -> Result<u64, WalletStorageError> {
        use crate::schema::accounts;

        let count = accounts::table
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("account_count", e))?;

        Ok(count as u64)
    }

    fn accounts_get_by_name(&mut self, name: &str) -> Result<Account, WalletStorageError> {
        use crate::schema::accounts;

        let row = accounts::table
            .filter(accounts::name.eq(name))
            .first::<models::Account>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("accounts_get_by_name", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "accounts_get_by_name",
                entity: "account".to_string(),
                key: name.to_string(),
            })?;

        let account = row.try_convert()?;

        Ok(account)
    }

    fn accounts_get_by_vault(&mut self, vault_id: &VaultId) -> Result<Account, WalletStorageError> {
        use crate::schema::{accounts, vaults};

        let account_id = vaults::table
            .select(vaults::account_id)
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("accounts_get_by_vault", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "accounts_get_by_vault",
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            })?;

        let row = accounts::table
            .filter(accounts::id.eq(account_id))
            .first::<models::Account>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("accounts_get_by_vault", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "accounts_get_by_vault",
                entity: "account".to_string(),
                key: vault_id.to_string(),
            })?;

        row.try_convert()
    }

    fn accounts_get_associated_stealth_resources(
        &mut self,
        address: &ComponentAddress,
    ) -> Result<HashSet<ResourceAddress>, WalletStorageError> {
        const OPERATION: &str = "accounts_get_associated_stealth_resources";
        use crate::schema::accounts;

        let stealth_resources = accounts::table
            .filter(accounts::address.eq(address.to_string()))
            .select(accounts::stealth_resources)
            .first::<String>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "account".to_string(),
                key: address.to_string(),
            })?;

        deserialize_json(&stealth_resources)
    }

    // -------------------------------- Vaults -------------------------------- //
    fn vaults_get(&mut self, vault_id: &VaultId) -> Result<VaultModel, WalletStorageError> {
        const OPERATION: &str = "vaults_get";
        use crate::schema::{accounts, vault_locks, vaults};

        let row = vaults::table
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<models::Vault>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "vault".to_string(),
                key: vault_id.to_string(),
            })?;

        let account_address = accounts::table
            .select(accounts::address)
            .filter(accounts::id.eq(row.account_id))
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let locked_revealed_balances = vault_locks::table
            .select(vault_locks::amount)
            .filter(vault_locks::vault_id.eq(row.id))
            .get_results::<String>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let locked_revealed_balance =
            locked_revealed_balances
                .into_iter()
                .try_fold(Amount::default(), |acc, amount_str| {
                    let amount = Amount::from_str(&amount_str).map_err(|e| WalletStorageError::DecodingError {
                        operation: OPERATION,
                        item: "vault lock amount",
                        details: format!("Failed to parse vault lock amount '{}': {}", amount_str, e),
                    })?;
                    Ok(acc + amount)
                })?;

        let component_addr =
            ComponentAddress::from_str(&account_address).map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "vault",
                details: e.to_string(),
            })?;
        let vault = row.try_into_vault(component_addr, locked_revealed_balance)?;
        Ok(vault)
    }

    fn vaults_exists(&mut self, vault_id: &VaultId) -> Result<bool, WalletStorageError> {
        use crate::schema::vaults;

        let count = vaults::table
            .filter(vaults::address.eq(vault_id.to_string()))
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_exists", e))?;

        Ok(count > 0)
    }

    fn vaults_get_by_resource(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
    ) -> Result<VaultModel, WalletStorageError> {
        const OPERATION: &str = "vaults_get_by_resource";
        use crate::schema::{accounts, vault_locks, vaults};

        let row = vaults::table
            .filter(
                vaults::account_id.eq(accounts::table
                    .filter(accounts::address.eq(account_addr.to_string()))
                    .select(accounts::id)
                    .single_value()
                    .assume_not_null()),
            )
            .filter(vaults::resource_address.eq(resource_address.to_string()))
            .first::<models::Vault>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "vault".to_string(),
                key: resource_address.to_string(),
            })?;

        let locked_revealed_balances = vault_locks::table
            .select(vault_locks::amount)
            .filter(vault_locks::vault_id.eq(row.id))
            .get_results::<String>(self.connection())
            .map_err(|e| WalletStorageError::general("vaults_get", e))?;
        let locked_revealed_balance =
            locked_revealed_balances
                .into_iter()
                .try_fold(Amount::default(), |acc, amount_str| {
                    let amount = Amount::from_str(&amount_str).map_err(|e| WalletStorageError::DecodingError {
                        operation: OPERATION,
                        item: "vault lock amount",
                        details: format!("Failed to parse vault lock amount '{}': {}", amount_str, e),
                    })?;
                    Ok(acc + amount)
                })?;

        let vault = row
            .try_into_vault(*account_addr, locked_revealed_balance)
            .map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "vault",
                details: format!("Failed to convert record to Vault: {}", e),
            })?;
        Ok(vault)
    }

    fn vaults_get_by_account(
        &mut self,
        account_addr: &ComponentAddress,
    ) -> Result<Vec<VaultModel>, WalletStorageError> {
        const OPERATION: &str = "vaults_get_by_account";
        use crate::schema::{accounts, vault_locks, vaults};

        let account_id = accounts::table
            .filter(accounts::address.eq(account_addr.to_string()))
            .select(accounts::id)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "account".to_string(),
                key: account_addr.to_string(),
            })?;

        let rows = vaults::table
            .filter(vaults::account_id.eq(account_id))
            .load::<models::Vault>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let vault_ids = rows.iter().map(|r| r.id);
        let lock_rows = vault_locks::table
            .select((vault_locks::vault_id, vault_locks::amount))
            .filter(vault_locks::vault_id.eq_any(vault_ids))
            .load_iter::<(i32, String), _>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let mut locked_balance_map = HashMap::new();
        for result in lock_rows {
            let (vault_id, amount_str) = result.map_err(|e| WalletStorageError::general(OPERATION, e))?;
            let amount = Amount::from_str(&amount_str).map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "vault lock amount",
                details: format!("Failed to parse vault lock amount '{}': {}", amount_str, e),
            })?;
            *locked_balance_map.entry(vault_id).or_default() += amount;
        }

        let vaults = rows
            .into_iter()
            .map(|row| {
                let locked_revealed_balance = locked_balance_map.remove(&row.id).unwrap_or_default();
                row.try_into_vault(*account_addr, locked_revealed_balance)
            })
            .collect::<Result<_, _>>()?;

        Ok(vaults)
    }

    fn vaults_get_ids_by_account(
        &mut self,
        account_addr: &ComponentAddress,
    ) -> Result<Vec<VaultId>, WalletStorageError> {
        const OPERATION: &str = "vaults_get_ids_by_account";
        use crate::schema::{accounts, vaults};

        let account_id = accounts::table
            .filter(accounts::address.eq(account_addr.to_string()))
            .select(accounts::id)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "account".to_string(),
                key: account_addr.to_string(),
            })?;

        let vault_addresses = vaults::table
            .filter(vaults::account_id.eq(account_id))
            .select(vaults::address)
            .get_results::<String>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        vault_addresses
            .into_iter()
            .map(|addr| {
                VaultId::from_str(&addr).map_err(|e| WalletStorageError::DecodingError {
                    operation: OPERATION,
                    item: "vault address",
                    details: e.to_string(),
                })
            })
            .collect()
    }

    // -------------------------------- Resources -------------------------------- //
    fn resources_get(&mut self, resource_address: &ResourceAddress) -> Result<ResourceModel, WalletStorageError> {
        const OPERATION: &str = "resources_get";

        use crate::schema::resources;

        let row = resources::table
            .filter(resources::address.eq(resource_address.to_string()))
            .first::<models::ResourceModel>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "resource".to_string(),
                key: resource_address.to_string(),
            })?;

        row.try_convert()
    }

    fn resources_get_by_type(&mut self, resource_type: ResourceType) -> Result<Vec<ResourceModel>, WalletStorageError> {
        const OPERATION: &str = "resources_get_by_type";

        use crate::schema::resources;

        let rows = resources::table
            .filter(resources::resource_type.eq(resource_type.to_string()))
            .get_results::<models::ResourceModel>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        rows.into_iter().map(|r| r.try_convert()).collect()
    }

    fn resources_get_many<'a, I: IntoIterator<Item = &'a ResourceAddress>>(
        &mut self,
        addresses: I,
    ) -> Result<Vec<ResourceModel>, WalletStorageError> {
        const OPERATION: &str = "resources_get_many";

        use crate::schema::resources;

        let rows = resources::table
            .filter(resources::address.eq_any(addresses.into_iter().map(|addr| addr.to_string())))
            .get_results::<models::ResourceModel>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        rows.into_iter().map(|r| r.try_convert()).collect()
    }

    // -------------------------------- Outputs -------------------------------- //
    fn confidential_outputs_get_unspent_balance(&mut self, vault_address: &VaultId) -> Result<u64, WalletStorageError> {
        use crate::schema::{confidential_outputs, vaults};

        let vault_id = vaults::table
            .filter(vaults::address.eq(vault_address.to_string()))
            .select(vaults::id)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("outputs_get_unspent_balance", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "outputs_get_unspent_balance",
                entity: "vault".to_string(),
                key: vault_address.to_string(),
            })?;

        let balance = confidential_outputs::table
            .select(sum(confidential_outputs::value))
            .filter(confidential_outputs::vault_id.eq(vault_id))
            .filter(confidential_outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            .first::<Option<BigDecimal>>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_get_unspent_balance", e))?;

        Ok(balance.map(|v| v.to_u64().expect("overflow")).unwrap_or(0))
    }

    fn confidential_outputs_get_locked_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
    ) -> Result<Vec<ConfidentialOutputModel>, WalletStorageError> {
        const OPERATION: &str = "outputs_get_locked_by_lock_id";
        use crate::schema::{accounts, confidential_outputs, vaults};

        let rows = confidential_outputs::table
            .filter(confidential_outputs::lock_id.eq(lock_id))
            .get_results::<models::ConfidentialOutput>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        if rows.is_empty() {
            return Ok(vec![]);
        }

        let vault_addresses = if rows.is_empty() {
            HashMap::new()
        } else {
            let vec = vaults::table
                .filter(vaults::id.eq_any(rows.iter().map(|v| v.vault_id)))
                .select((vaults::id, vaults::address))
                .get_results::<(i32, String)>(self.connection())
                .map_err(|e| WalletStorageError::general("outputs_get_locked_by_proof", e))?;
            vec.into_iter().collect()
        };

        // account_id should be the same in all rows
        let account_address = rows
            .first()
            .map(|row| {
                accounts::table
                    .filter(accounts::id.eq(row.account_id))
                    .select(accounts::address)
                    .first::<String>(self.connection())
                    .map_err(|e| WalletStorageError::general("outputs_get_locked_by_proof", e))
            })
            .transpose()?;

        let confidential_outputs = rows
            .into_iter()
            .map(|row| {
                let vault_id = row.vault_id;
                row.try_into_output(
                    account_address.as_ref().unwrap(),
                    vault_addresses.get(&vault_id).unwrap(),
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(confidential_outputs)
    }

    fn confidential_outputs_get_by_commitment(
        &mut self,
        vault_id: &VaultId,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<ConfidentialOutputModel, WalletStorageError> {
        use crate::schema::{accounts, confidential_outputs, vaults};

        let row = confidential_outputs::table
            .filter(
                confidential_outputs::vault_id.eq(vaults::table
                    .select(vaults::id)
                    .filter(vaults::address.eq(vault_id.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
            )
            .filter(confidential_outputs::commitment.eq(serialize_hex(commitment)))
            .first::<models::ConfidentialOutput>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("outputs_get_by_commitment", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "outputs_get_by_commitment",
                entity: "output".to_string(),
                key: serialize_hex(commitment),
            })?;

        let account_addr = accounts::table
            .filter(accounts::id.eq(row.account_id))
            .select(accounts::address)
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_get_by_commitment", e))?;

        let vaults_addr = vaults::table
            .filter(vaults::id.eq(row.vault_id))
            .select(vaults::address)
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_get_by_commitment", e))?;

        let output = row.try_into_output(&account_addr, &vaults_addr)?;
        Ok(output)
    }

    fn confidential_outputs_get_by_account_and_status(
        &mut self,
        account_addr: &ComponentAddress,
        status: OutputStatus,
    ) -> Result<Vec<ConfidentialOutputModel>, WalletStorageError> {
        use crate::schema::{accounts, confidential_outputs, vaults};

        let account_id = accounts::table
            .filter(accounts::address.eq(account_addr.to_string()))
            .select(accounts::id)
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_get_by_account_and_status", e))?;

        let rows = confidential_outputs::table
            .filter(confidential_outputs::account_id.eq(account_id))
            .filter(confidential_outputs::status.eq(status.as_key_str()))
            .load::<models::ConfidentialOutput>(self.connection())
            .map_err(|e| WalletStorageError::general("outputs_get_by_account_and_status", e))?;

        let vault_addresses = if rows.is_empty() {
            HashMap::new()
        } else {
            let vec = vaults::table
                .filter(vaults::id.eq_any(rows.iter().map(|v| v.vault_id)))
                .select((vaults::id, vaults::address))
                .get_results::<(i32, String)>(self.connection())
                .map_err(|e| WalletStorageError::general("outputs_get_locked_by_proof", e))?;
            vec.into_iter().collect()
        };

        let outputs = rows
            .into_iter()
            .map(|row| {
                let vault_id = row.vault_id;
                row.try_into_output(&account_addr.to_string(), vault_addresses.get(&vault_id).unwrap())
            })
            .collect::<Result<_, _>>()?;
        Ok(outputs)
    }

    fn stealth_outputs_get_unspent_balance(
        &mut self,
        resource_address: &ResourceAddress,
    ) -> Result<StealthBalance, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_get_unspent_balance";
        use crate::schema::stealth_outputs;

        let (balance, utxo_count) = stealth_outputs::table
            .select((
                dsl::sum(dsl::sql::<sql_types::BigInt>("CAST(value as LONG)")),
                dsl::count(stealth_outputs::id),
            ))
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            .first::<(Option<BigDecimal>, i64)>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let balance = balance
            .map(|v| v.to_u128().expect("BUG: stealth amount overflow"))
            .unwrap_or(0)
            .into();

        Ok(StealthBalance {
            balance,
            // Negative count should be impossible
            utxo_count: usize::try_from(utxo_count)
                .expect("INVARIANT: negative or utxo count > usize:::MAX should not be possible in SQLite"),
        })
    }

    fn stealth_outputs_count_by_status(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
        status: OutputStatus,
    ) -> Result<u64, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_count_by_status";
        use crate::schema::stealth_outputs;

        let count = stealth_outputs::table
            .filter(
                stealth_outputs::owner_account_id.eq(accounts::table
                    .select(accounts::id)
                    .filter(accounts::address.eq(account_addr.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
            )
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(stealth_outputs::status.eq(status.as_key_str()))
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        Ok(count as u64)
    }

    fn stealth_outputs_get_unspent_by_account(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: Option<&ResourceAddress>,
        exclude_locked: bool,
    ) -> Result<Vec<StealthOutputInfo>, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_get_all_by_account";
        use crate::schema::{accounts, stealth_outputs};

        let mut query = stealth_outputs::table
            .filter(
                stealth_outputs::owner_account_id.eq(accounts::table
                    .select(accounts::id)
                    .filter(accounts::address.eq(account_addr.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
            )
            .filter(stealth_outputs::status.eq(OutputStatus::Unspent.as_key_str()))
            .into_boxed();

        if let Some(resource_address) = resource_address {
            query = query.filter(stealth_outputs::resource_address.eq(resource_address.to_string()));
        }

        if exclude_locked {
            query = query.filter(stealth_outputs::lock_id.is_null());
        }

        let rows = query
            .load_iter::<models::StealthOutput, _>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        rows.map(|row| {
            row.map_err(|e| WalletStorageError::general(OPERATION, e))
                .and_then(|row| row.try_into())
        })
        .collect()
    }

    fn stealth_outputs_get_unspent_for_spending(
        &mut self,
        account_addr: &ComponentAddress,
        resource_address: &ResourceAddress,
        lock_id: WalletLockId,
    ) -> Result<Vec<StealthOutputInfo>, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_get_unspent_for_spending";
        use crate::schema::{accounts, stealth_outputs};

        let rows = stealth_outputs::table
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(
                stealth_outputs::owner_account_id.eq(accounts::table
                    .select(accounts::id)
                    .filter(accounts::address.eq(account_addr.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
            )
            .filter(
                stealth_outputs::status
                    .eq(OutputStatus::Unspent.as_key_str())
                    // Also include outputs created within the transaction
                    .or(stealth_outputs::status
                        .eq(OutputStatus::LockedUnconfirmed.as_key_str())
                        .and(stealth_outputs::lock_id.eq(lock_id))),
            )
            .filter(stealth_outputs::owner_key_id.is_not_null())
            .filter(stealth_outputs::is_burnt.eq(false))
            .filter(stealth_outputs::is_condition_spendable.eq(true))
            .filter(stealth_outputs::is_frozen.eq(false))
            .load_iter::<models::StealthOutput, _>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        rows.map(|row| {
            row.map_err(|e| WalletStorageError::general(OPERATION, e))
                .and_then(|row| row.try_into())
        })
        .collect()
    }

    fn stealth_outputs_get_locked_by_lock_id(
        &mut self,
        lock_id: WalletLockId,
    ) -> Result<Vec<StealthOutputModel>, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_get_locked_by_lock_id";
        use crate::schema::{accounts, stealth_outputs};

        let rows = stealth_outputs::table
            .filter(stealth_outputs::lock_id.eq(lock_id))
            .get_results::<models::StealthOutput>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        // account_id should be the same in all rows
        let first_row = rows.first().ok_or_else(|| WalletStorageError::NotFound {
            operation: OPERATION,
            entity: "stealth_output".to_string(),
            key: lock_id.to_string(),
        })?;
        let account_address = accounts::table
            .filter(accounts::id.eq(first_row.owner_account_id))
            .select(accounts::address)
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let account_address = account_address.parse().map_err(|e| WalletStorageError::DecodingError {
            operation: OPERATION,
            item: "account",
            details: format!("Corrupt db: invalid owner account address '{account_address}': {e}"),
        })?;

        rows.into_iter()
            .map(|row| row.try_convert(account_address))
            .collect::<Result<_, _>>()
    }

    fn stealth_outputs_get_by_commitment(
        &mut self,
        resource_address: &ResourceAddress,
        commitment: &PedersenCommitmentBytes,
    ) -> Result<StealthOutputModel, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_get_by_commitment";
        use crate::schema::stealth_outputs;

        let row = stealth_outputs::table
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .filter(stealth_outputs::commitment.eq(serialize_hex(commitment)))
            .first::<models::StealthOutput>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: OPERATION,
                entity: "output".to_string(),
                key: serialize_hex(commitment),
            })?;

        let account_address = accounts::table
            .filter(accounts::id.eq(row.owner_account_id))
            .select(accounts::address)
            .first::<String>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?
            .parse()
            .map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "account",
                details: format!("Corrupt db: invalid owner account address: {e}"),
            })?;

        row.try_convert(account_address)
    }

    fn stealth_outputs_get_many(
        &mut self,
        resource_address: &ResourceAddress,
        by_account: Option<&ComponentAddress>,
        by_status: Option<OutputStatus>,
    ) -> Result<Vec<StealthOutputModel>, WalletStorageError> {
        const OPERATION: &str = "stealth_outputs_get_many";
        use crate::schema::{accounts, stealth_outputs};

        let mut query = stealth_outputs::table
            .inner_join(accounts::table.on(accounts::id.eq(stealth_outputs::owner_account_id)))
            .select((stealth_outputs::all_columns, accounts::address))
            .filter(stealth_outputs::resource_address.eq(resource_address.to_string()))
            .into_boxed();

        if let Some(account_addr) = by_account {
            // NOTE: because we also join on accounts table, we cannot use it in the filter directly due to a diesel
            // limitation. Also tried aliases, but didn't immediately work and decided this is fine.
            let account_id = accounts::table
                .select(accounts::id)
                .filter(accounts::address.eq(account_addr.to_string()))
                .limit(1)
                .get_result::<i32>(self.connection())
                .map_err(|e| WalletStorageError::general(OPERATION, e))?;
            query = query.filter(stealth_outputs::owner_account_id.eq(account_id));
        }

        if let Some(status) = by_status {
            query = query.filter(stealth_outputs::status.eq(status.as_key_str()));
        }

        let rows = query
            .order_by(stealth_outputs::id.desc())
            .get_results::<(models::StealthOutput, String)>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        rows.into_iter()
            .map(|(row, address)| {
                let address = address.parse().map_err(|e| WalletStorageError::DecodingError {
                    operation: OPERATION,
                    item: "account",
                    details: format!("Corrupt db: invalid owner account address '{address}': {e}"),
                })?;
                row.try_convert(address)
            })
            .collect()
    }

    fn locks_get_by_transaction_id(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<WalletLockId, WalletStorageError> {
        use crate::schema::locks;

        let lock_id = locks::table
            .filter(locks::transaction_id.eq(serialize_hex(transaction_id)))
            .select(locks::id)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("locks_get_by_transaction_id", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "locks_get_by_transaction_id",
                entity: "locks".to_string(),
                key: serialize_hex(transaction_id),
            })?;

        Ok(lock_id)
    }

    fn non_fungible_token_get_by_nft_id(
        &mut self,
        resource_address: ResourceAddress,
        nft_id: NonFungibleId,
    ) -> Result<NonFungibleToken, WalletStorageError> {
        use crate::schema::{non_fungible_tokens, vaults};

        let non_fungible_token = non_fungible_tokens::table
            .filter(non_fungible_tokens::resource_id.eq(resource_address.to_string()))
            .filter(non_fungible_tokens::nft_id.eq(nft_id.to_string()))
            .first::<crate::models::NonFungibleToken>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("non_fungible_token_get_by_nft_id", e))?;
        let non_fungible_token = non_fungible_token.ok_or_else(|| WalletStorageError::NotFound {
            operation: "non_fungible_token_get_by_nft_id",
            entity: "non_fungible_tokens".to_string(),
            key: nft_id.to_string(),
        })?;

        let vault_id = non_fungible_token.vault_id;
        let vault_address = vaults::table
            .select(vaults::address)
            .filter(vaults::id.eq(vault_id))
            .first::<String>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("accounts_get_by_vault", e))?
            .ok_or_else(|| WalletStorageError::NotFound {
                operation: "non_fungible_token_get_by_nft_id",
                entity: "non_fungible_tokens".to_string(),
                key: format!("{}", vault_id),
            })?;
        let vault_address = VaultId::from_str(&vault_address).map_err(|e| WalletStorageError::DecodingError {
            details: e.to_string(),
            item: "non_fungible_tokens",
            operation: "non_fungible_token_get_by_nft_id",
        })?;
        non_fungible_token.try_into_non_fungible_token(vault_address)
    }

    fn non_fungible_token_get_ids_by_vault_id(
        &mut self,
        vault_id: &VaultId,
        limit: u64,
        offset: u64,
    ) -> Result<HashSet<NonFungibleId>, WalletStorageError> {
        const OPERATION: &str = "non_fungible_token_get_ids_by_vault_id";
        use crate::schema::{non_fungible_tokens, vaults};

        let vault_id = vaults::table
            .select(vaults::id)
            .filter(vaults::address.eq(vault_id.to_string()))
            .first::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let non_fungibles = non_fungible_tokens::table
            .select(non_fungible_tokens::nft_id)
            .filter(non_fungible_tokens::vault_id.eq(vault_id))
            .limit(limit as i64)
            .offset(offset as i64)
            .get_results::<String>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        non_fungibles
            .into_iter()
            .map(|nft_id| {
                NonFungibleId::try_from_canonical_string(&nft_id).map_err(|e| WalletStorageError::DecodingError {
                    operation: OPERATION,
                    item: "non_fungible_tokens.nft_id",
                    details: format!("{:?}", e),
                })
            })
            .collect()
    }

    fn non_fungible_token_get_all(
        &mut self,
        account: ComponentAddress,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<NonFungibleToken>, WalletStorageError> {
        use crate::schema::{accounts, non_fungible_tokens, vaults};

        let vault_ids = vaults::table
            .select(vaults::id)
            .left_join(accounts::table.on(accounts::id.eq(vaults::account_id)))
            .filter(accounts::address.eq(account.to_string()))
            .get_results::<i32>(self.connection())
            .map_err(|e| WalletStorageError::general("non_fungible_token_get_all", e))?;

        let non_fungibles = non_fungible_tokens::table
            .left_join(vaults::table.on(vaults::id.eq(non_fungible_tokens::vault_id)))
            .select((non_fungible_tokens::all_columns, vaults::all_columns.nullable()))
            .filter(non_fungible_tokens::vault_id.eq_any(vault_ids))
            .limit(limit as i64)
            .offset(offset as i64)
            .load::<(models::NonFungibleToken, Option<models::Vault>)>(self.connection())
            .map_err(|e| WalletStorageError::general("non_fungible_token_get_all", e))?;

        non_fungibles
            .into_iter()
            .map(|(n, vault)| {
                let vault = vault.ok_or_else(|| WalletStorageError::DataInconsistent {
                    operation: "non_fungible_token_get_all",
                    details: format!("Vault not found for nft: {}", n.nft_id),
                })?;
                let vault_id = VaultId::from_str(&vault.address).map_err(|e| WalletStorageError::DecodingError {
                    details: format!("Failed to convert vault address to VaultId: {}", e),
                    item: "vault_id",
                    operation: "non_fungible_token_get_all",
                })?;
                n.try_into_non_fungible_token(vault_id)
            })
            .collect()
    }

    fn non_fungible_token_get_resource_address(
        &mut self,
        nft_id: NonFungibleId,
    ) -> Result<ResourceAddress, WalletStorageError> {
        use crate::schema::{non_fungible_tokens, vaults};

        let vault_id = non_fungible_tokens::table
            .filter(non_fungible_tokens::nft_id.eq(nft_id.to_string()))
            .select(non_fungible_tokens::vault_id)
            .first::<i32>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("non_fungible_token_get_resource_address", e))?;
        let vault_id = vault_id.ok_or_else(|| WalletStorageError::NotFound {
            operation: "non_fungible_token_get_resource_address",
            entity: "non_fungible_tokens".to_string(),
            key: nft_id.to_string(),
        })?;

        let resource_address = vaults::table
            .filter(vaults::id.eq(vault_id))
            .select(vaults::resource_address)
            .first::<String>(self.connection())
            .optional()
            .map_err(|e| WalletStorageError::general("non_fungible_token_get_resource_address", e))?;
        let resource_address = resource_address.ok_or_else(|| WalletStorageError::NotFound {
            operation: "non_fungible_token_get_resource_address",
            entity: "non_fungible_tokens".to_string(),
            key: nft_id.to_string(),
        })?;

        ResourceAddress::from_str(&resource_address).map_err(|e| WalletStorageError::DecodingError {
            item: "non_fungible_tokens",
            operation: "non_fungible_token_get_resource_address",
            details: e.to_string(),
        })
    }

    fn webauthn_is_user_registered(&mut self, username: &str) -> Result<bool, WalletStorageError> {
        use crate::schema::webauthn_registrations;
        let count: i64 = webauthn_registrations::table
            .count()
            .filter(webauthn_registrations::username.eq(username))
            .limit(1)
            .get_result(self.connection())
            .map_err(|e| WalletStorageError::general("webauthn_reg_count", e))?;
        Ok(count > 0)
    }

    fn webauthn_reg_fetch_passkeys(&mut self, username: String) -> Result<Vec<Passkey>, WalletStorageError> {
        use crate::schema::{webauthn_registration_passkeys, webauthn_registrations};
        Ok(webauthn_registration_passkeys::table
            .inner_join(webauthn_registrations::table)
            .filter(webauthn_registrations::username.eq(username))
            .select(WebauthnRegistrationPasskey::as_select())
            .load::<WebauthnRegistrationPasskey>(self.connection())
            .map_err(|e| WalletStorageError::general("webauthn_reg_fetch", e))?
            .iter()
            .filter_map(|model| match WebauthnRegistrationPasskeyModel::try_from(model) {
                Ok(value) => Some(value.passkey),
                Err(_) => None,
            })
            .collect::<Vec<Passkey>>())
    }

    fn authored_templates_exists_by_address(&mut self, address: &TemplateAddress) -> Result<bool, WalletStorageError> {
        use crate::schema::authored_templates;
        let address_hex = format!("{}", address);
        let address_count = authored_templates::table
            .filter(authored_templates::address.eq(address_hex))
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("authored_templates_exists_by_address", e))?;
        Ok(address_count > 0)
    }

    fn authored_templates_fetch_by_public_key(
        &mut self,
        author_public_key: &RistrettoPublicKeyBytes,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<AuthoredTemplateModel>, u64), WalletStorageError> {
        use crate::schema::authored_templates;

        let author_public_key_str = serialize_hex(author_public_key);

        let total_templates_for_key_index = authored_templates::table
            .filter(authored_templates::author_public_key.eq(&author_public_key_str))
            .count()
            .first::<i64>(self.connection())
            .map_err(|e| WalletStorageError::general("count_authored_templates_fetch_by_key_index", e))?;

        let templates = authored_templates::table
            .filter(authored_templates::author_public_key.eq(author_public_key_str))
            .limit(page_size as i64)
            .offset((page * page_size) as i64)
            .select(AuthoredTemplate::as_select())
            .load::<AuthoredTemplate>(self.connection())
            .map_err(|e| WalletStorageError::general("authored_templates_fetch_by_key_index", e))?
            .iter()
            .filter_map(|model| match AuthoredTemplateModel::try_from(model) {
                Ok(model) => Some(model),
                Err(error) => {
                    warn!(target: LOG_TARGET, "Invalid authored template record found: {:?}", error);
                    None
                },
            })
            .collect::<Vec<AuthoredTemplateModel>>();

        Ok((templates, total_templates_for_key_index as u64))
    }

    fn shard_state_version_get(
        &mut self,
        account: &ComponentAddress,
        resource: &ResourceAddress,
    ) -> Result<HashMap<Shard, StateVersion>, WalletStorageError> {
        const OPERATION: &str = "shard_state_version_get";
        use crate::schema::{accounts, resources, shard_state_versions};

        let row = shard_state_versions::table
            .select((shard_state_versions::shard, shard_state_versions::state_version))
            .filter(
                shard_state_versions::account_id.eq(accounts::table
                    .select(accounts::id)
                    .filter(accounts::address.eq(account.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
            )
            .filter(
                shard_state_versions::resource_id.eq(resources::table
                    .select(resources::id)
                    .filter(resources::address.eq(resource.to_string()))
                    .limit(1)
                    .single_value()
                    .assume_not_null()),
            )
            .get_results::<(i32, i64)>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let mut versions = HashMap::with_capacity(row.len());
        for (shard, version) in row {
            versions.insert(Shard::from(shard as u32), StateVersion::new(version as u64));
        }
        Ok(versions)
    }

    fn utxo_process_queue_fetch_batch(
        &mut self,
        batch_size: usize,
    ) -> Result<HashMap<ResourceAddress, HashMap<TagAndPublicNoncePair, ComponentAddress>>, WalletStorageError> {
        const OPERATION: &str = "utxo_process_queue_fetch_batch";
        use crate::schema::{accounts, utxo_process_queue};

        let rows = utxo_process_queue::table
            .inner_join(accounts::table.on(accounts::id.eq(utxo_process_queue::account_id)))
            .select((utxo_process_queue::all_columns, accounts::address.assume_not_null()))
            .order(utxo_process_queue::id.asc())
            .limit(i64::try_from(batch_size).unwrap_or(i64::MAX))
            .get_results::<(models::UtxoProcessQueue, String)>(self.connection())
            .map_err(|e| WalletStorageError::general(OPERATION, e))?;

        let mut result = HashMap::new();
        for (row, account_addr) in &rows {
            let resource_address =
                ResourceAddress::from_str(&row.resource_address).map_err(|e| WalletStorageError::DecodingError {
                    operation: OPERATION,
                    item: "resource_address",
                    details: format!("Corrupt db: invalid resource address '{}': {}", row.resource_address, e),
                })?;
            let tag = UtxoTag::new(row.utxo_tag as u32);
            let public_nonce = deserialize_hex_try_from(&row.public_nonce)?;
            let account_addr = account_addr.parse().map_err(|e| WalletStorageError::DecodingError {
                operation: OPERATION,
                item: "account_address",
                details: format!("Corrupt db: invalid account address '{}': {}", account_addr, e),
            })?;
            result
                .entry(resource_address)
                .or_insert_with(HashMap::new)
                .insert((tag, public_nonce), account_addr);
        }

        Ok(result)
    }
}

impl Drop for ReadTransaction<'_> {
    fn drop(&mut self) {
        if !self.is_done &&
            let Err(err) = self.rollback_internal()
        {
            error!(target: LOG_TARGET, "Failed to rollback transaction: {}", err);
        }
    }
}
