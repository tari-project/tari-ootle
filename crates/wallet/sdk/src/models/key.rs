//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, str::FromStr};

use tari_bor::{Deserialize, Serialize};
use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_address::RistrettoOotleAddress;
use tari_template_lib::prelude::RistrettoPublicKeyBytes;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-daemon-client/"))]
#[serde(rename_all = "snake_case")]
pub enum KeyBranch {
    /// The account key branch, used for deriving account keys.
    Account,
    /// The transaction key branch, used to sign transactions that do not need to be signed with the account key.
    Transaction,
    /// The Elgamal encryption view key branch, used to derive a view key for resources with "viewable balance"
    /// enabled.
    ElgamalEncryptionViewKey,
    /// The stealth mask branch, used to derive masks for stealth addresses.
    StealthMask,
    /// The confidential mask branch, used to derive masks for confidential transactions.
    ConfidentialMask,
    /// Used to generate nonces that need to be recreated later, e.g. to derive the DH secret for claim burn
    Nonce,
    /// Branch used to derive view-only keys. This key is used to derive an encryption key for wallet recovery. But
    /// does not allow spending.
    ViewOnlyKey,
}

impl KeyBranch {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Transaction => "transactions",
            Self::ElgamalEncryptionViewKey => "elgamal_view_key",
            Self::StealthMask => "stealth_mask",
            Self::ConfidentialMask => "confidential_mask",
            Self::Nonce => "nonce",
            Self::ViewOnlyKey => "view_only_key",
        }
    }
}

impl AsRef<str> for KeyBranch {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone)]
pub struct WalletKeyRecord {
    pub(crate) key_id: KeyId,
    pub(crate) public_key: RistrettoPublicKey,
    pub(crate) secret_key: RistrettoSecretKey,
    pub(crate) is_active: bool,
}

impl WalletKeyRecord {
    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletOotleAddressWithKeyIds {
    pub address: RistrettoOotleAddress,
    pub view_only_key_id: KeyId,
    pub owner_key_id: KeyId,
}

#[derive(Clone)]
pub struct ImportedWalletKey {
    pub key: RistrettoSecretKey,
    pub import_id: ImportedKeyId,
}

impl ImportedWalletKey {
    pub fn to_public_key(&self) -> RistrettoPublicKey {
        RistrettoPublicKey::from_secret_key(&self.key)
    }

    pub fn as_key_id(&self) -> KeyId {
        KeyId::imported(self.import_id)
    }
}

#[derive(Clone)]
pub struct DerivedWalletKey {
    pub key: RistrettoSecretKey,
    pub key_index: DerivedKeyIndex,
}

impl DerivedWalletKey {
    pub fn to_public_key(&self) -> RistrettoPublicKey {
        RistrettoPublicKey::from_secret_key(&self.key)
    }

    pub fn as_key_id(&self) -> KeyId {
        KeyId::derived(self.key_index)
    }
}

impl From<tari_transaction_components::key_manager::tari_key_manager::DerivedKey> for DerivedWalletKey {
    fn from(key: tari_transaction_components::key_manager::tari_key_manager::DerivedKey) -> Self {
        Self {
            key: key.key,
            key_index: key.key_index,
        }
    }
}

#[derive(Clone)]
pub struct WalletPublicKey {
    pub public_key: RistrettoPublicKey,
    pub key_id: KeyId,
}

impl WalletPublicKey {
    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    pub fn key_id(&self) -> KeyId {
        self.key_id
    }
}

impl From<DerivedWalletKey> for WalletPublicKey {
    fn from(derived: DerivedWalletKey) -> Self {
        Self {
            key_id: derived.as_key_id(),
            public_key: derived.to_public_key(),
        }
    }
}

#[derive(Clone)]
pub struct WalletSecretKey {
    pub secret: RistrettoSecretKey,
    pub key_id: KeyId,
}

impl WalletSecretKey {
    pub fn secret(&self) -> &RistrettoSecretKey {
        &self.secret
    }

    pub fn key_id(&self) -> &KeyId {
        &self.key_id
    }

    pub fn to_public_key(&self) -> RistrettoPublicKey {
        RistrettoPublicKey::from_secret_key(&self.secret)
    }
}

impl From<DerivedKeyPair> for WalletSecretKey {
    fn from(pair: DerivedKeyPair) -> Self {
        Self {
            key_id: pair.derived_key.as_key_id(),
            secret: pair.derived_key.key,
        }
    }
}

impl From<DerivedWalletKey> for WalletSecretKey {
    fn from(derived: DerivedWalletKey) -> Self {
        Self {
            key_id: derived.as_key_id(),
            secret: derived.key,
        }
    }
}

impl From<ImportedWalletKey> for WalletSecretKey {
    fn from(imported: ImportedWalletKey) -> Self {
        Self {
            key_id: imported.as_key_id(),
            secret: imported.key,
        }
    }
}

impl From<WalletKeyRecord> for WalletSecretKey {
    fn from(record: WalletKeyRecord) -> Self {
        Self {
            secret: record.secret_key,
            key_id: record.key_id,
        }
    }
}

#[derive(Clone)]
pub struct AccountAndViewKeys {
    pub account_public_key: RistrettoPublicKeyBytes,
    pub account_key: Option<WalletSecretKey>,
    pub view_only_key: WalletSecretKey,
}

#[derive(Clone)]
pub struct DerivedKeyPair {
    pub public_key: RistrettoPublicKey,
    pub derived_key: DerivedWalletKey,
}

impl DerivedKeyPair {
    pub fn key_index(&self) -> DerivedKeyIndex {
        self.derived_key.key_index
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.public_key
    }

    pub fn secret_key(&self) -> &RistrettoSecretKey {
        &self.derived_key.key
    }
}

pub type DerivedKeyIndex = u64;
pub type ImportedKeyId = u64;

#[derive(Debug, Clone, Copy)]
pub enum KeyType {
    /// View only key
    ViewOnly,
    /// Owner key, allows spending and write access to components
    Owner,
    /// General purpose key, can be used for any purpose
    GeneralPurpose,
}

impl Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ViewOnly => write!(f, "ViewOnly"),
            Self::Owner => write!(f, "Owner"),
            Self::GeneralPurpose => write!(f, "GeneralPurpose"),
        }
    }
}

impl FromStr for KeyType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ViewOnly" => Ok(Self::ViewOnly),
            "Owner" => Ok(Self::Owner),
            "GeneralPurpose" => Ok(Self::GeneralPurpose),
            _ => Err(anyhow::anyhow!("Invalid key type: {}", s)),
        }
    }
}

pub enum KeyIdOrPublicKey {
    KeyId(KeyId),
    PublicKey(RistrettoPublicKeyBytes),
}

impl From<KeyId> for KeyIdOrPublicKey {
    fn from(key_id: KeyId) -> Self {
        Self::KeyId(key_id)
    }
}

impl From<RistrettoPublicKeyBytes> for KeyIdOrPublicKey {
    fn from(public_key: RistrettoPublicKeyBytes) -> Self {
        Self::PublicKey(public_key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-daemon-client/"))]
pub enum KeyId {
    /// Derived from the seed key
    Derived { index: DerivedKeyIndex },
    /// Imported key
    Imported { local_key_id: ImportedKeyId },
}

impl KeyId {
    pub fn derived(index: DerivedKeyIndex) -> Self {
        Self::Derived { index }
    }

    pub fn imported(local_key_id: ImportedKeyId) -> Self {
        Self::Imported { local_key_id }
    }

    pub fn derived_index(&self) -> Option<DerivedKeyIndex> {
        match self {
            Self::Derived { index } => Some(*index),
            Self::Imported { .. } => None,
        }
    }

    pub fn imported_key_id(&self) -> Option<ImportedKeyId> {
        match self {
            Self::Imported { local_key_id } => Some(*local_key_id),
            Self::Derived { .. } => None,
        }
    }
}

impl Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Derived { index } => write!(f, "Derived({index})"),
            Self::Imported {
                local_key_id: local_import_id,
            } => write!(f, "Imported({local_import_id})"),
        }
    }
}
