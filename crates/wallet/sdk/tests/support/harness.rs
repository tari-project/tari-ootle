//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, convert::Infallible, future::Future, str::FromStr};

use tari_crypto::tari_utilities::SafePassword;
use tari_engine_types::{
    crypto::commit_amount_checked,
    substate::{Substate, SubstateId},
    ToByteType,
    Utxo,
};
use tari_ootle_common_types::{optional::Optional, shard::Shard, Network, StateVersion};
use tari_ootle_wallet_sdk::{
    models::{ConfidentialOutputModel, OutputStatus, UtxoUpdateSet, WalletLockId},
    network::{SubstateQueryResult, TransactionQueryResult, WalletNetworkInterface},
    storage::TagAndPublicNoncePair,
    WalletSdk,
    WalletSdkConfig,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_template_abi::TemplateDef;
use tari_template_lib::{
    constants::STEALTH_TARI_RESOURCE_ADDRESS,
    models::{ComponentAddress, EncryptedData, ResourceAddress, UtxoId, VaultId},
    prelude::{Amount, PedersenCommitmentBytes, ResourceType, TemplateAddress},
};
use tari_transaction::{Transaction, TransactionId};

pub struct Test {
    store: SqliteWalletStore,
    sdk: WalletSdk<SqliteWalletStore, PanicNetworkInterface>,
    _temp: tempfile::TempDir,
}

impl Test {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = SqliteWalletStore::try_open(temp.path().join("data/wallet.sqlite")).unwrap();
        store.run_migrations().unwrap();

        let mut sdk = WalletSdk::initialize(store.clone(), PanicNetworkInterface, WalletSdkConfig {
            network: Network::LocalNet,
            override_keyring_password: Some(SafePassword::from_str("SuuuCh Sekret W0W").unwrap()),
        })
        .unwrap();
        sdk.initialize_cipher_seed(None).unwrap();
        let accounts_api = sdk.accounts_api();
        accounts_api
            .add_account(Some("test"), &Test::test_account_address(), 0, true, true)
            .unwrap();
        accounts_api
            .add_vault(
                Test::test_account_address(),
                Test::test_vault_address(),
                STEALTH_TARI_RESOURCE_ADDRESS,
                ResourceType::Stealth,
                Some("TEST".to_string()),
                6,
            )
            .unwrap();

        Self {
            store,
            sdk,
            _temp: temp,
        }
    }

    pub fn test_account_address() -> ComponentAddress {
        "component_0dc41b5cc74b36d696c7b140323a40a2f98b71df5d60e5a6bf4c1a07ffffffff"
            .parse()
            .unwrap()
    }

    pub fn test_vault_address() -> VaultId {
        "vault_0dc41b5cc74b36d696c7b140323a40a2f98b71df5d60e5a6bf4c1a07ffffffff"
            .parse()
            .unwrap()
    }

    pub fn add_unspent_output<A: Into<Amount>>(&self, amount: A) -> PedersenCommitmentBytes {
        let amount = amount.into();

        let outputs_api = self.sdk.confidential_outputs_api();
        let commitment = commit_amount_checked(&Default::default(), amount)
            .expect("Cannot add unspent output with negative amount")
            .to_byte_type();
        outputs_api
            .add_output(ConfidentialOutputModel {
                account_address: Self::test_account_address(),
                vault_id: Self::test_vault_address(),
                commitment,
                value: amount,
                sender_public_nonce: None,
                encryption_secret_key_index: 0,
                encrypted_data: EncryptedData::try_from(vec![0; EncryptedData::min_size()]).unwrap(),
                public_asset_tag: None,
                status: OutputStatus::Unspent,
                lock_id: None,
            })
            .unwrap();
        commitment
    }

    pub fn new_lock(&self) -> WalletLockId {
        self.sdk.confidential_outputs_api().create_lock().unwrap()
    }

    pub fn get_unspent_balance(&self) -> Amount {
        let outputs_api = self.sdk.confidential_outputs_api();
        outputs_api
            .get_unspent_balance(&Test::test_vault_address())
            .optional()
            .unwrap()
            .unwrap_or_default()
    }

    pub fn sdk(&self) -> &WalletSdk<SqliteWalletStore, PanicNetworkInterface> {
        &self.sdk
    }

    pub fn store(&self) -> &SqliteWalletStore {
        &self.store
    }
}

#[derive(Debug, Clone)]
pub struct PanicNetworkInterface;

// TODO: test the substate scanning in the SDK
impl WalletNetworkInterface for PanicNetworkInterface {
    type Error = Infallible;

    #[allow(clippy::diverging_sub_expression)]
    async fn query_substate(
        &self,
        _address: &SubstateId,
        _version: Option<u32>,
        _local_search_only: bool,
    ) -> Result<SubstateQueryResult, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    async fn get_substates(&self, _: Vec<SubstateId>) -> Result<HashMap<SubstateId, Substate>, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    #[allow(clippy::diverging_sub_expression)]
    async fn submit_transaction(&self, _transaction: Transaction) -> Result<TransactionId, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    #[allow(clippy::diverging_sub_expression)]
    async fn submit_dry_run_transaction(
        &self,
        _transaction: Transaction,
    ) -> Result<TransactionQueryResult, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    #[allow(clippy::diverging_sub_expression)]
    async fn query_transaction_result(
        &self,
        _transaction_id: TransactionId,
    ) -> Result<TransactionQueryResult, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    async fn fetch_template_definition(&self, _template_address: TemplateAddress) -> Result<TemplateDef, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    async fn query_stealth_utxo_updates(
        &self,
        _resource_address: ResourceAddress,
        _shard_state_versions: HashMap<Shard, StateVersion>,
    ) -> Result<UtxoUpdateSet, Self::Error> {
        panic!("PanicNetworkInterface called")
    }

    async fn get_unspent_utxos(
        &self,
        _resource_address: ResourceAddress,
        _tag_and_nonce_pairs: Vec<TagAndPublicNoncePair>,
    ) -> Result<Vec<(UtxoId, Utxo)>, Self::Error> {
        panic!("PanicNetworkInterface get_unspent_utxos called")
    }

    async fn wait_until_ready(&self) -> Result<(), Self::Error> {
        panic!("PanicNetworkInterface called")
    }
}
