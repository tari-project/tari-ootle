//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use digest::crypto_common::rand_core::{OsRng, RngCore};
use log::{info, warn};
use passwords::PasswordGenerator;
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
        resources::ResourcesApi,
        stealth_crypto::StealthCryptoApi,
        stealth_outputs::StealthOutputsApi,
        stealth_transfer::StealthTransferApi,
        substate::SubstatesApi,
        template::TemplateApi,
        transaction::TransactionApi,
    },
    network::{StatusResponseError, WalletNetworkInterface},
    storage::{WalletStorageError, WalletStore},
};

const KEYRING_ENTRIES_SERVICE: &str = "tari-ootle-wallet";
const CIPHER_SEED_PASSWORD_KEYRING_ENTRY_NAME: &str = "cipher-seed-password";

const LOG_TARGET: &str = "wallet::sdk::api";

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
    loaded_cipher_seed: Option<Arc<CipherSeed>>,
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
        let config_api = ConfigApi::new(&store);
        if !config_api.exists(ConfigKey::Network)? {
            config_api.set(ConfigKey::Network, config.network.as_key_str(), false)?;
        }

        Ok(Self {
            store,
            network_interface: indexer,
            config,
            loaded_cipher_seed: None,
        })
    }

    /// Initializes the cipher seed for the wallet. Either creating a new cipher seed or recovering it from the provided
    /// seed words if provided and necessary. Returns true if the cipher seed was recovered from the seed words,
    /// otherwise false.
    pub fn initialize_cipher_seed(&mut self, seed_words: Option<&SeedWords>) -> Result<bool, WalletSdkError> {
        match self.load_cipher_seed()? {
            Some(_) => {
                if seed_words.is_some() {
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
            None => {
                if let Some(seed_words) = seed_words {
                    self.restore_cipher_seed(seed_words)?;
                    info!(target: LOG_TARGET, "🔑 Successfully restored wallet seed key!");
                    self.config_api().set(ConfigKey::RecoveryNeeded, &true, false)?;
                    Ok(true)
                } else {
                    self.create_cipher_seed()?;
                    self.config_api().set(ConfigKey::RecoveryNeeded, &false, false)?;
                    Ok(false)
                }
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
    ///
    /// ## Panics
    /// This function will panic if the cipher seed has not been initialized i.e. `initialize_cipher_seed` has not been
    /// called once before calling this.
    pub fn key_manager_api(&self) -> KeyManagerApi<'_, TStore> {
        KeyManagerApi::new(
            &self.store,
            self.loaded_cipher_seed
                .as_ref()
                .expect("key_manager_api: cipher seed not initialized. initialize_cipher_seed must be called first"),
        )
    }

    pub fn transaction_api(&self) -> TransactionApi<'_, TStore, TNetworkInterface> {
        TransactionApi::new(&self.store, &self.network_interface)
    }

    pub fn substate_api(&self) -> SubstatesApi<'_, TStore, TNetworkInterface> {
        SubstatesApi::new(&self.store, &self.network_interface)
    }

    pub fn accounts_api(&self) -> AccountsApi<'_, TStore, TNetworkInterface> {
        AccountsApi::new(&self.store, self.substate_api(), self.key_manager_api())
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
            self.key_manager_api(),
            self.accounts_api(),
            self.stealth_outputs_api(),
            self.substate_api(),
            self.stealth_crypto_api(),
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

    /// Tries to get encrypted cipher seed from DB and decrypts it using OS keyring if possible.
    fn load_cipher_seed(&mut self) -> Result<Option<Arc<CipherSeed>>, WalletSdkError> {
        if let Some(ref cipher_seed) = self.loaded_cipher_seed {
            return Ok(Some(cipher_seed.clone()));
        }

        let Some(cipher_seed_encrypted) = self.config_api().get::<Vec<u8>>(ConfigKey::CipherSeed).optional()? else {
            // Cipher seed not found in DB. This is expected if the wallet has not been initialized yet.
            return Ok(None);
        };
        let password = self.get_cipher_seed_password()?;
        let cipher_seed = CipherSeed::from_enciphered_bytes(&cipher_seed_encrypted, Some(password))?;
        self.loaded_cipher_seed = Some(Arc::new(cipher_seed));
        Ok(self.loaded_cipher_seed.clone())
    }

    fn create_cipher_seed(&mut self) -> Result<(), WalletSdkError> {
        let cipher_seed = CipherSeed::new();
        let password = self.create_cipher_seed_password()?;
        let encrypted_cipher_seed = cipher_seed.encipher(Some(password))?;
        self.config_api()
            .set(ConfigKey::CipherSeed, &encrypted_cipher_seed, true)?;
        self.loaded_cipher_seed = Some(Arc::new(cipher_seed));
        Ok(())
    }

    /// Restores cipher seed from seed words, encrypts with a new random password (and saves to OS keychain)
    /// and replaces current cipher seed in the DB (to let every component use the new seed).
    fn restore_cipher_seed(&mut self, seed_words: &SeedWords) -> Result<(), WalletSdkError> {
        let cipher_seed = CipherSeed::from_mnemonic(seed_words, None)?;
        let password = self.create_cipher_seed_password()?;
        let encrypted_cipher_seed = cipher_seed.encipher(Some(password))?;
        self.config_api()
            .set(ConfigKey::CipherSeed, &encrypted_cipher_seed, true)?;
        self.loaded_cipher_seed = Some(Arc::new(cipher_seed));
        Ok(())
    }

    /// Retrieve the seed words from current cipher seed stored.
    pub fn load_seed_words(&mut self) -> Result<SeedWords, WalletSdkError> {
        let seed_words = self
            .load_cipher_seed()?
            .ok_or_else(|| WalletSdkError::InvariantError {
                details: "call to load_cipher_seed without initializing the cipher seed".to_string(),
            })?
            .to_mnemonic(MnemonicLanguage::English, None)?;
        Ok(seed_words)
    }

    fn get_cipher_seed_password(&self) -> Result<SafePassword, WalletSdkError> {
        if let Some(ref password) = self.config.override_keyring_password {
            return Ok(password.clone());
        }

        let key = self.config_api().get::<String>(ConfigKey::KeyringPasswordEntryKey)?;
        let entry = self.get_cipher_seed_password_keyring_entry(&key)?;
        // If get_password fails with NoEntry, it means that the password is not set in the keyring i.e. IsNotFoundError
        // will return true which is what we want.
        let password = entry.get_password()?;
        Ok(SafePassword::from(password))
    }

    fn create_cipher_seed_password(&mut self) -> Result<SafePassword, WalletSdkError> {
        if let Some(ref password) = self.config.override_keyring_password {
            // If we are overriding the keyring password, we don't need to set it in the keyring.
            // This is because the password is already set in the config.
            return Ok(password.clone());
        }

        let key = match self
            .config_api()
            .get::<String>(ConfigKey::KeyringPasswordEntryKey)
            .optional()?
        {
            Some(key) => key,
            None => {
                // If the key is not set, we generate a new key and set it in the config.
                // The nonce is used to differentiate between different password entries in the keyring when running
                // multiple instances of the wallet on the same network. This nonce is generated once per wallet
                // database.
                let nonce = generate_password_entry_key_nonce();
                let key = format!(
                    "{}-{}-{}",
                    CIPHER_SEED_PASSWORD_KEYRING_ENTRY_NAME, self.config.network, nonce
                );
                self.config_api().set(ConfigKey::KeyringPasswordEntryKey, &key, false)?;
                key
            },
        };

        let (str_password, safe_password) = generate_password()?;
        let entry = self.get_cipher_seed_password_keyring_entry(&key)?;
        entry.set_password(&str_password)?;
        Ok(safe_password)
    }

    fn get_cipher_seed_password_keyring_entry(&self, key: &str) -> Result<keyring::Entry, WalletSdkError> {
        let result = keyring::Entry::new(KEYRING_ENTRIES_SERVICE, key);

        match result {
            Ok(entry) => Ok(entry),
            Err(keyring::Error::NoEntry) => {
                // NoEntry maps to various errors in the keyring codebase, including AccessDenied, keyExpired etc.
                // Entry::new says that it will only return an error if the service/user are invalid but there may be
                // more errors possible e.g. AccessDenied. In any case we provide a better error than NoEntry for this
                // case. We dont want IsNotFoundError to be true for this case.
                Err(WalletSdkError::FailedToAccessKeyRing)
            },
            Err(err) => Err(err.into()),
        }
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
    KeyManager(#[from] KeyManagerApiError),
    #[error("Cipher error: {0}")]
    CipherError(#[from] CipherError),
    #[error("Failed to generate password for cipher seed: {0}")]
    PasswordGeneration(String),
    #[error(
        "OS keyring not supported on this device. You may have to specify an encryption password by using the \
         `--password` cli option."
    )]
    FailedToAccessKeyRing,
    #[error(transparent)]
    NetworkParseError(#[from] NetworkParseError),
    #[error("Invariant error: {details}. This indicates a bug in the code.")]
    InvariantError { details: String },
}

impl IsNotFoundError for WalletSdkError {
    fn is_not_found_error(&self) -> bool {
        match self {
            Self::WalletStorageError(e) => e.is_not_found_error(),
            Self::ConfigApiError(e) => e.is_not_found_error(),
            Self::KeyManager(e) => e.is_not_found_error(),
            Self::KeyRing(keyring::Error::NoEntry) => true,
            Self::KeyRing(_) |
            Self::CipherError(_) |
            Self::PasswordGeneration(_) |
            Self::InvariantError { .. } |
            Self::FailedToAccessKeyRing |
            Self::NetworkParseError(_) => false,
        }
    }
}

// Generate a new random password.
fn generate_password() -> Result<(Zeroizing<String>, SafePassword), WalletSdkError> {
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

    let safe_password = SafePassword::from(generated_password.clone());
    Ok((Zeroizing::new(generated_password), safe_password))
}

fn generate_password_entry_key_nonce() -> u64 {
    OsRng.next_u64()
}
