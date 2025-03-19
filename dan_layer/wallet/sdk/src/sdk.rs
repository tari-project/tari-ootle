//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, sync::Arc, time::Duration};

use keyring::Entry;
use log::info;
use passwords::PasswordGenerator;
use tari_common::{configuration::Network, ConfigurationError};
use tari_crypto::tari_utilities::SafePassword;
use tari_dan_common_types::optional::{IsNotFoundError, Optional};
use tari_key_manager::{
    cipher_seed::CipherSeed,
    error::KeyManagerError,
    mnemonic::{Mnemonic, MnemonicLanguage},
    SeedWords,
};

use crate::{
    apis::{
        accounts::AccountsApi,
        confidential_crypto::ConfidentialCryptoApi,
        confidential_outputs::ConfidentialOutputsApi,
        confidential_transfer::ConfidentialTransferApi,
        config::{ConfigApi, ConfigApiError, ConfigKey},
        jwt::JwtApi,
        key_manager::KeyManagerApi,
        non_fungible_tokens::NonFungibleTokensApi,
        substate::SubstatesApi,
        transaction::TransactionApi,
    },
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore},
};

const KEYRING_ENTRIES_SERVICE: &str = "tari-ootle-wallet";
const CIPHER_SEED_PASSWORD_KEYRING_ENTRY_NAME: &str = "cipher-seed-password";

const LOG_TARGET: &str = "wallet::sdk::api";

#[derive(Debug, Clone)]
pub struct WalletSdkConfig {
    /// Encryption password for the wallet database. NOTE: Not yet implemented, this field is ignored
    pub password: Option<SafePassword>,
    // TODO: remove JWT stuff from wallet SDK. The SDK should not have anything to do with JWTs, this is a web/jrpc
    //       handler concern. It appears that the main reason it is done this way is to use the wallet database to
    //       store JWT state. However this can be achieved by calling the _SQLite_ (non-abstract) store directly
    // outside       of the SDK in the JWT handler.
    pub jwt_expiry: Duration,
    pub jwt_secret_key: String,
}

#[derive(Debug, Clone)]
pub struct DanWalletSdk<TStore, TNetworkInterface> {
    store: TStore,
    network_interface: TNetworkInterface,
    config: WalletSdkConfig,
    cipher_seed: Arc<CipherSeed>,
}

impl<TStore, TNetworkInterface> DanWalletSdk<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn initialize(
        network: Network,
        store: TStore,
        indexer: TNetworkInterface,
        config: WalletSdkConfig,
        seed_words: Option<SeedWords>,
        wallet_secret: Option<String>,
    ) -> Result<Self, WalletSdkError> {
        // initialize network
        let config_api = ConfigApi::new(&store);
        if !config_api.exists(ConfigKey::Network)? {
            config_api.set(ConfigKey::Network, network.as_key_str(), false)?;
        }

        // initialize cipher seed
        let cipher_seed = match Self::cipher_seed(&store)? {
            Some(cipher_seed) => {
                if seed_words.is_some() {
                    Err(WalletSdkError::AlreadyInitialized)
                } else {
                    Ok(cipher_seed)
                }
            },
            None => {
                if let Some(seed_words) = seed_words {
                    let result = Self::restore_cipher_seed(&store, &seed_words, wallet_secret);
                    if result.is_ok() {
                        info!(target: LOG_TARGET, "🔑 Successfully restored wallet seed key!");
                        // TODO: trigger/return indicator that scan is needed for resources (account etc...)
                    }
                    result
                } else {
                    Self::create_cipher_seed(&store, wallet_secret)
                }
            },
        }?;

        Ok(Self {
            store,
            network_interface: indexer,
            config,
            cipher_seed: Arc::new(cipher_seed),
        })
    }

    pub fn config_api(&self) -> ConfigApi<'_, TStore> {
        ConfigApi::new(&self.store)
    }

    pub fn get_config(&self) -> &WalletSdkConfig {
        &self.config
    }

    pub fn get_network_interface(&self) -> &TNetworkInterface {
        &self.network_interface
    }

    pub fn get_network_interface_mut(&mut self) -> &mut TNetworkInterface {
        &mut self.network_interface
    }

    pub fn key_manager_api(&self) -> KeyManagerApi<'_, TStore> {
        KeyManagerApi::new(&self.store, &self.cipher_seed)
    }

    pub fn transaction_api(&self) -> TransactionApi<'_, TStore, TNetworkInterface> {
        TransactionApi::new(&self.store, &self.network_interface)
    }

    pub fn substate_api(&self) -> SubstatesApi<'_, TStore, TNetworkInterface> {
        SubstatesApi::new(&self.store, &self.network_interface)
    }

    pub fn accounts_api(&self) -> AccountsApi<'_, TStore> {
        AccountsApi::new(&self.store)
    }

    pub fn confidential_crypto_api(&self) -> ConfidentialCryptoApi {
        ConfidentialCryptoApi::new()
    }

    pub fn jwt_api(&self) -> JwtApi<'_, TStore> {
        JwtApi::new(&self.store, self.config.jwt_expiry, self.config.jwt_secret_key.clone())
    }

    pub fn confidential_outputs_api(&self) -> ConfidentialOutputsApi<'_, TStore> {
        ConfidentialOutputsApi::new(
            &self.store,
            self.key_manager_api(),
            self.accounts_api(),
            self.confidential_crypto_api(),
        )
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

    pub fn non_fungible_api(&self) -> NonFungibleTokensApi<'_, TStore> {
        NonFungibleTokensApi::new(&self.store)
    }

    fn cipher_seed_password_keyring_entry(network: Network) -> Result<Entry, keyring::Error> {
        Entry::new(
            KEYRING_ENTRIES_SERVICE,
            format!("{}-{}", network, CIPHER_SEED_PASSWORD_KEYRING_ENTRY_NAME).as_str(),
        )
    }

    /// Tries to get encrypted cipher seed from DB and decrypts it using OS keyring if possible.
    fn cipher_seed(store: &TStore) -> Result<Option<CipherSeed>, WalletSdkError> {
        let config_api = ConfigApi::new(store);
        let network = Network::from_str(config_api.get::<String>(ConfigKey::Network)?.as_str())?;
        let cipher_seed_encrypted: Option<Vec<u8>> = config_api.get(ConfigKey::CipherSeed).optional()?;
        if cipher_seed_encrypted.is_none() {
            return Ok(None);
        }
        let cipher_seed_encrypted = cipher_seed_encrypted.unwrap();
        let entry = Self::cipher_seed_password_keyring_entry(network)?;
        match entry.get_password() {
            Ok(raw_password) => {
                let password =
                    SafePassword::from_str(raw_password.as_str()).map_err(|_| WalletSdkError::SafePassword)?;
                let cipher_seed = CipherSeed::from_enciphered_bytes(&cipher_seed_encrypted, Some(password))?;
                Ok(Some(cipher_seed))
            },
            Err(keyring::Error::NoEntry) => {
                // if we have no entry found in OS keyring it means that,
                // we did not create the cipher seed yet, so it's not an error
                Ok(None)
            },
            Err(error) => Err(error.into()),
        }
    }

    // Generate a new random password.
    fn generate_password() -> Result<(String, SafePassword), WalletSdkError> {
        let pg = PasswordGenerator {
            length: 256,
            numbers: true,
            lowercase_letters: true,
            uppercase_letters: true,
            symbols: false,
            spaces: false,
            exclude_similar_characters: false,
            strict: true,
        };
        let generated_password = pg
            .generate_one()
            .map_err(|error| WalletSdkError::PasswordGeneration(error.to_string()))?;
        Ok((
            generated_password.clone(),
            SafePassword::from_str(generated_password.as_str()).map_err(|_| WalletSdkError::SafePassword)?,
        ))
    }

    fn create_cipher_seed(store: &TStore, password_opt: Option<String>) -> Result<CipherSeed, WalletSdkError> {
        let config_api = ConfigApi::new(store);
        let network = Network::from_str(config_api.get::<String>(ConfigKey::Network)?.as_str())?;
        let cipher_seed = CipherSeed::new();
        let (password_raw, password) = if let Some(pass) = password_opt {
            (
                pass.clone(),
                SafePassword::from_str(pass.as_str()).map_err(|_| WalletSdkError::SafePassword)?,
            )
        } else {
            Self::generate_password()?
        };
        let entry = Self::cipher_seed_password_keyring_entry(network)?;
        entry.set_password(password_raw.as_str())?;
        let encrypted_cipher_seed = cipher_seed.encipher(Some(password))?;
        config_api.set(ConfigKey::CipherSeed, &encrypted_cipher_seed, true)?;
        Ok(cipher_seed)
    }

    /// Restores cipher seed from seed words, encrypts with a new random password (and saves to OS keychain)
    /// and replaces current cipher seed in the DB (to let every component use the new seed).
    pub fn restore_cipher_seed(
        store: &TStore,
        seed_words: &SeedWords,
        password_opt: Option<String>,
    ) -> Result<CipherSeed, WalletSdkError> {
        let cipher_seed = CipherSeed::from_mnemonic(seed_words, None)?;
        let config_api = ConfigApi::new(store);
        let network = Network::from_str(config_api.get::<String>(ConfigKey::Network)?.as_str())?;
        let (password_raw, password) = if let Some(pass) = password_opt {
            (
                pass.clone(),
                SafePassword::from_str(pass.as_str()).map_err(|_| WalletSdkError::SafePassword)?,
            )
        } else {
            Self::generate_password()?
        };
        let entry = Self::cipher_seed_password_keyring_entry(network)?;
        entry.set_password(password_raw.as_str())?;
        let encrypted_cipher_seed = cipher_seed.encipher(Some(password))?;
        config_api.set(ConfigKey::CipherSeed, &encrypted_cipher_seed, true)?;
        Ok(cipher_seed)
    }

    /// Retrieve the seed words from current cipher seed stored.
    pub fn seed_words(&self) -> Result<SeedWords, WalletSdkError> {
        Ok(Self::cipher_seed(&self.store)?
            .ok_or(WalletSdkError::NoCipherSeed)?
            .to_mnemonic(MnemonicLanguage::English, None)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletSdkError {
    #[error("Wallet storage error: {0}")]
    WalletStorageError(#[from] WalletStorageError),
    #[error("Config API error: {0}")]
    ConfigApiError(#[from] ConfigApiError),
    #[error("OS Keyring error: {0}")]
    KeyRing(#[from] keyring::Error),
    #[error("Key manager error: {0}")]
    KeyManager(#[from] KeyManagerError),
    #[error("Failed to get safe password")]
    SafePassword,
    #[error("No cipher seed present")]
    NoCipherSeed,
    #[error("Failed to generate password for cipher seed: {0}")]
    PasswordGeneration(String),
    #[error("Not able to restore wallet from seed words as this wallet is already initialized!")]
    AlreadyInitialized,
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigurationError),
}
