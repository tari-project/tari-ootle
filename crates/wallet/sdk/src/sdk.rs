//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::{info, warn};
use tari_common_types::seeds::{
    cipher_seed::CipherSeed,
    error::CipherError,
    mnemonic::{Mnemonic, MnemonicLanguage},
    seed_words::SeedWords,
};
use tari_crypto::tari_utilities::SafePassword;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
    NetworkParseError,
};
use zeroize::Zeroizing;

use crate::{
    apis::{
        accounts::AccountsApi,
        confidential_crypto::ConfidentialCryptoApi,
        confidential_outputs::ConfidentialOutputsApi,
        confidential_transfer::ConfidentialTransferApi,
        config::{ConfigApi, ConfigApiError, ConfigKey},
        key_manager::{KeyManagerApi, KeyManagerApiError},
        non_fungible_tokens::NonFungibleTokensApi,
        password_manager::{PasswordManagerApi, PasswordManagerApiError},
        resources::ResourcesApi,
        signer::SignerApi,
        stealth_crypto::StealthCryptoApi,
        stealth_outputs::StealthOutputsApi,
        stealth_transfer::StealthTransferApi,
        substate::SubstatesApi,
        template::TemplateApi,
        transaction::TransactionApi,
        viewable_balance::ViewableBalanceApi,
    },
    cipher_seed::{CipherSeedRestore, WalletCipherSeed},
    key_managers::local::LocalKeyManager,
    local_key_store::LocalKeyStore,
    network::{StatusResponseError, WalletNetworkInterface},
    storage::{WalletStorageError, WalletStore},
};

const LOG_TARGET: &str = "wallet::sdk::api";

pub type LocalSignerApi<'a, TStore> = SignerApi<LocalKeyManager<LocalKeyStore<'a, TStore>>>;

#[derive(Debug, Clone)]
pub struct WalletSdkConfig {
    pub network: Network,
    /// Encryption password for the wallet database.
    pub override_keyring_password: Option<SafePassword>,
}

#[derive(Debug, Clone)]
pub struct WalletSdk<TStore, TNetworkInterface> {
    store: TStore,
    network_interface: TNetworkInterface,
    config: WalletSdkConfig,
    loaded_cipher_seed: WalletCipherSeed,
}

impl<TStore, TNetworkInterface> WalletSdk<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub fn initialize(
        store: TStore,
        indexer: TNetworkInterface,
        config: WalletSdkConfig,
    ) -> Result<WalletSdk<TStore, TNetworkInterface>, WalletSdkError> {
        // initialize network
        if let Some(network) = Self::get_store_network(&store)? {
            if config.network != network {
                return Err(WalletSdkError::InvariantError {
                    details: format!(
                        "Network mismatch. Config network is {:?} but database network is {:?}",
                        config.network, network
                    ),
                });
            }
        } else {
            ConfigApi::new(&store).set(ConfigKey::Network, config.network.as_key_str())?;
        }

        Ok(Self {
            store,
            network_interface: indexer,
            config,
            loaded_cipher_seed: WalletCipherSeed::None,
        })
    }

    pub fn get_store_network(store: &TStore) -> Result<Option<Network>, WalletSdkError> {
        let config_api = ConfigApi::new(store);
        let network = config_api.get(ConfigKey::Network).optional()?;
        Ok(network)
    }

    /// Initializes the cipher seed for the wallet. Either creating a new cipher seed or recovering it from the provided
    /// seed words if provided and necessary. Returns true if the cipher seed was recovered from the seed words,
    /// otherwise false.
    pub fn initialize_cipher_seed(&mut self, restore: CipherSeedRestore<'_>) -> Result<bool, WalletSdkError> {
        match self.load_cipher_seed()? {
            Some(_) => {
                if !restore.is_create_new() {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️  Wallet already initialized. Ignoring seed words provided for recovery.",
                    );
                }
                let requires_recovery = self.config_api().get(ConfigKey::RecoveryNeeded).optional()?;
                // This should have been set - it is an error if it is not
                requires_recovery.ok_or_else(|| WalletSdkError::InvariantError {
                    details: "Cipher seed already initialized but recovery_needed not set.".to_string(),
                })
            },
            None => match restore {
                CipherSeedRestore::CreateNewIfRequired => {
                    self.create_cipher_seed()?;
                    self.config_api().set(ConfigKey::RecoveryNeeded, &false)?;
                    Ok(false)
                },
                CipherSeedRestore::FromSeedWords(seed_words) => {
                    self.restore_cipher_seed_from_seed_words(seed_words)?;
                    info!(target: LOG_TARGET, "🔑 Successfully restored wallet seed key!");
                    self.config_api().set(ConfigKey::RecoveryNeeded, &true)?;
                    Ok(true)
                },
            },
        }
    }

    pub fn store(&self) -> &TStore {
        &self.store
    }

    pub fn config_api(&self) -> ConfigApi<'_, TStore> {
        ConfigApi::new(&self.store)
    }

    pub fn sdk_config(&self) -> &WalletSdkConfig {
        &self.config
    }

    pub fn network(&self) -> Network {
        self.config.network
    }

    pub fn get_network_interface(&self) -> &TNetworkInterface {
        &self.network_interface
    }

    /// Returns the KeyManager API for the wallet.
    pub fn key_manager_api(&self) -> KeyManagerApi<'_, TStore> {
        let network = self.config.network;
        KeyManagerApi::new(
            network,
            &self.store,
            LocalKeyStore::new(&self.loaded_cipher_seed, self.password_manager_api(), &self.store),
            self.password_manager_api(),
        )
    }

    /// Returns the Signer API for the wallet if the cipher seed has been initialized. This signer uses the local key
    /// store where key material is kept in the local database.
    pub fn local_signer_api(&self) -> LocalSignerApi<'_, TStore> {
        let store = LocalKeyStore::new(&self.loaded_cipher_seed, self.password_manager_api(), &self.store);
        let backend = LocalKeyManager::new(store);
        SignerApi::new(backend)
    }

    pub(crate) fn password_manager_api(&self) -> PasswordManagerApi<'_, TStore> {
        PasswordManagerApi::new(self.config_api(), &self.config)
    }

    pub fn transaction_api(&self) -> TransactionApi<'_, TStore, TNetworkInterface> {
        TransactionApi::new(&self.store, &self.network_interface)
    }

    pub fn substate_api(&self) -> SubstatesApi<'_, TStore, TNetworkInterface> {
        SubstatesApi::new(&self.store, &self.network_interface)
    }

    pub fn accounts_api(&self) -> AccountsApi<'_, TStore, TNetworkInterface> {
        AccountsApi::new(
            self.config.network,
            &self.store,
            self.substate_api(),
            self.key_manager_api(),
        )
    }

    pub fn resources_api(&self) -> ResourcesApi<'_, TStore> {
        ResourcesApi::new(&self.store)
    }

    pub fn confidential_crypto_api(&self) -> ConfidentialCryptoApi {
        ConfidentialCryptoApi::new()
    }

    pub fn confidential_outputs_api(&self) -> ConfidentialOutputsApi<'_, TStore> {
        ConfidentialOutputsApi::new(&self.store, self.key_manager_api(), self.confidential_crypto_api())
    }

    pub fn confidential_transfer_api(&self) -> ConfidentialTransferApi<'_, TStore, TNetworkInterface> {
        ConfidentialTransferApi::new(
            self.key_manager_api(),
            self.accounts_api(),
            self.confidential_outputs_api(),
            self.substate_api(),
            self.confidential_crypto_api(),
            self.config_api(),
        )
    }

    pub fn stealth_crypto_api(&self) -> StealthCryptoApi {
        StealthCryptoApi::new()
    }

    pub fn stealth_transfer_api(&self) -> StealthTransferApi<'_, TStore, TNetworkInterface> {
        StealthTransferApi::new(
            self.accounts_api(),
            self.stealth_outputs_api(),
            self.substate_api(),
            self.config_api(),
        )
    }

    pub fn stealth_outputs_api(&self) -> StealthOutputsApi<'_, TStore> {
        StealthOutputsApi::new(
            self.store(),
            self.key_manager_api(),
            self.stealth_crypto_api(),
            self.config_api(),
        )
    }

    pub fn non_fungible_api(&self) -> NonFungibleTokensApi<'_, TStore> {
        NonFungibleTokensApi::new(&self.store)
    }

    pub fn template_api(&self) -> TemplateApi<'_, TStore> {
        TemplateApi::new(&self.store)
    }

    pub fn viewable_balance_api(&self) -> ViewableBalanceApi {
        ViewableBalanceApi
    }

    /// Tries to get encrypted cipher seed from DB and decrypts it using OS keyring if possible.
    fn load_cipher_seed(&mut self) -> Result<Option<&CipherSeed>, WalletSdkError> {
        // Workaround for borrow checker limitation as described in https://blog.polybdenum.com/2024/12/21/four-limitations-of-rust-s-borrow-checker.html
        if self.loaded_cipher_seed.cipher_seed().is_some() {
            return Ok(Some(self.loaded_cipher_seed.cipher_seed().expect("checked above")));
        }

        let Some(cipher_seed_encrypted) = self
            .config_api()
            .get::<Zeroizing<Vec<u8>>>(ConfigKey::CipherSeed)
            .optional()?
        else {
            // Cipher seed not found in DB. This is expected if the wallet has not been initialized yet.
            return Ok(None);
        };
        let password = self.password_manager_api().get_cipher_seed_password()?;
        let cipher_seed = CipherSeed::from_enciphered_bytes(&cipher_seed_encrypted, Some(password))?;
        self.loaded_cipher_seed = cipher_seed.into();
        Ok(self.loaded_cipher_seed.cipher_seed())
    }

    fn create_cipher_seed(&mut self) -> Result<(), WalletSdkError> {
        let password = self.password_manager_api().create_cipher_seed_password()?;
        let cipher_seed = CipherSeed::new();
        let encrypted_cipher_seed = cipher_seed.encipher(Some(password))?;
        self.config_api().set(ConfigKey::CipherSeed, &encrypted_cipher_seed)?;
        self.loaded_cipher_seed = cipher_seed.into();
        Ok(())
    }

    /// Restores cipher seed from seed words, encrypts with a new random password (and saves to OS keychain)
    /// and replaces current cipher seed in the DB (to let every component use the new seed).
    fn restore_cipher_seed_from_seed_words(&mut self, seed_words: &SeedWords) -> Result<(), WalletSdkError> {
        let password = self.password_manager_api().create_cipher_seed_password()?;
        let cipher_seed = CipherSeed::from_mnemonic(seed_words, None)?;
        let encrypted_cipher_seed = cipher_seed.encipher(Some(password))?;
        self.config_api().set(ConfigKey::CipherSeed, &encrypted_cipher_seed)?;
        self.loaded_cipher_seed = cipher_seed.into();
        Ok(())
    }

    /// Retrieve the seed words from current cipher seed stored.
    pub fn load_seed_words(&mut self) -> Result<Option<SeedWords>, WalletSdkError> {
        let seed_words = self
            .load_cipher_seed()?
            .map(|s| s.to_mnemonic(MnemonicLanguage::English, None))
            .transpose()?;
        Ok(seed_words)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletSdkError {
    #[error("Wallet storage error: {0}")]
    WalletStorageError(#[from] WalletStorageError),
    #[error("Config API error: {0}")]
    ConfigApiError(#[from] ConfigApiError),
    #[error("Key manager error: {0}")]
    KeyManager(#[from] KeyManagerApiError),
    #[error("Cipher error: {0}")]
    CipherError(#[from] CipherError),
    #[error("Password manager error: {0}")]
    PasswordManagerError(#[from] PasswordManagerApiError),
    #[error(transparent)]
    NetworkParseError(#[from] NetworkParseError),
    #[error("Invariant error: {details}. This indicates a bug in the code.")]
    InvariantError { details: String },
}
