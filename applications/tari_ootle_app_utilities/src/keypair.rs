//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Keypair management for Tari node identities.
//!
//! This module provides functionality for creating, loading, and securely storing cryptographic keypairs
//! used for node identification in the Tari network.
//!
//! # Security Considerations
//!
//! ## File Permissions (CRITICAL)
//!
//! **On Unix-like systems (Linux, macOS):** Identity files containing private keys are protected with
//! strict file permissions (0600 - read/write for owner only). These permissions are enforced both when
//! creating new identity files and when loading existing ones.
//!
//! **On Windows systems:** File permission enforcement is **NOT IMPLEMENTED**. The Windows permission
//! model differs significantly from Unix, and this module does not currently set or verify Windows ACLs.
//!
//! ### Security Implications for Windows Users
//!
//! - Identity files on Windows are created with default permissions, which may allow access by other users on the
//!   system.
//! - On shared or multi-user Windows systems, **private keys may be exposed** to unauthorized users.
//! - Users running on Windows should manually secure their identity files using Windows File Explorer or `icacls`
//!   command to restrict access to their user account only.
//!
//! ### Recommended Windows Security Steps
//!
//! To manually secure your identity file on Windows:
//! 1. Right-click the identity file → Properties → Security tab
//! 2. Click "Advanced" → Disable inheritance → Remove all users except your account
//! 3. Ensure only your user account has "Full Control"
//!
//! Alternatively, use PowerShell:
//! ```powershell
//! $path = "path\to\identity_file.json"
//! icacls $path /inheritance:r
//! icacls $path /grant:r "$env:USERNAME:(F)"
//! ```

use std::{fs, io, path::Path, sync::Arc};

use log::*;
use rand::{CryptoRng, Rng};
use serde::{Serialize, de::DeserializeOwned};
use tari_common::{
    configuration::bootstrap::prompt,
    exit_codes::{ExitCode, ExitError},
};
use tari_common_types::seeds::{
    cipher_seed::CipherSeed,
    error::{CipherError, MnemonicError},
    mnemonic::{Mnemonic, MnemonicLanguage},
    seed_words::SeedWords,
};
use tari_crypto::{
    keys::{PublicKey as _, SecretKey as _},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
#[cfg(feature = "p2p")]
use tari_ootle_p2p::PeerAddress;
use tari_ootle_wallet_crypto::derive_ristretto_key;
use tari_utilities::SafePassword;

const REQUIRED_IDENTITY_PERMS: u32 = 0o100600;
const LOG_TARGET: &str = "tari::application::identity";

/// Key derivation branch for Tari validator node identity keypairs.
pub const VN_IDENTITY_BRANCH: &[u8] = b"validator_node";
/// Key derivation branch for Tari validator node fee claim keypairs.
pub const VN_FEE_CLAIM_BRANCH: &[u8] = b"fee_claim";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RistrettoKeypair(Arc<KeyPairInner>);

impl RistrettoKeypair {
    pub fn random<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        let secret_key = RistrettoSecretKey::random(rng);
        Self::from_secret_key(secret_key)
    }

    pub fn from_secret_key(secret_key: RistrettoSecretKey) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
        Self(Arc::new(KeyPairInner { secret_key, public_key }))
    }

    /// Derives a keypair deterministically from a cipher seed, branch, and index.
    pub fn derive_from_seed(seed: &CipherSeed, branch: &[u8], index: u64) -> Self {
        let secret_key = derive_ristretto_key(seed.entropy(), branch, index);
        Self::from_secret_key(secret_key)
    }

    /// Derives a keypair deterministically from a 24-word seed phrase, optional passphrase, branch, and index.
    pub fn derive_from_seed_words(
        seed_words: &SeedWords,
        passphrase: Option<SafePassword>,
        branch: &[u8],
        index: u64,
    ) -> Result<Self, IdentityError> {
        let seed = CipherSeed::from_mnemonic(seed_words, passphrase)?;
        Ok(Self::derive_from_seed(&seed, branch, index))
    }

    pub fn secret_key(&self) -> &RistrettoSecretKey {
        &self.0.secret_key
    }

    pub fn public_key(&self) -> &RistrettoPublicKey {
        &self.0.public_key
    }

    #[cfg(feature = "p2p")]
    pub fn to_peer_address(&self) -> PeerAddress {
        self.public_key().clone().into()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KeyPairInner {
    secret_key: RistrettoSecretKey,
    public_key: RistrettoPublicKey,
}

/// Loads the node identity, or creates a new one if create_id is true
///
/// ## Parameters
/// - `identity_file` - Reference to file path
/// - `create_id` - Only applies if the identity_file does not exist or is malformed. If true, a new identity will be
///   created, otherwise the user will be prompted to create a new ID
///
/// # Return
/// A NodeIdentity wrapped in an atomic reference counter on success, the exit code indicating the reason on failure
pub fn setup_keypair_prompt<P: AsRef<Path>>(identity_file: P, create_id: bool) -> Result<RistrettoKeypair, ExitError> {
    setup_keypair_from_seed_prompt(identity_file, None, None, create_id)
}

/// Sets up a node identity, supporting restore from seed words when provided, or creating a fresh seed-derived
/// identity.
///
/// ## Parameters
/// - `identity_file` - Reference to file path
/// - `seed_words` - Optional 24-word seed phrase for deterministic recovery
/// - `passphrase` - Optional passphrase for seed decryption
/// - `create_id` - If true, creates a new seed and identity if the file does not exist
pub fn setup_keypair_from_seed_prompt<P: AsRef<Path>>(
    identity_file: P,
    seed_words: Option<&SeedWords>,
    passphrase: Option<SafePassword>,
    create_id: bool,
) -> Result<RistrettoKeypair, ExitError> {
    if let Some(words) = seed_words {
        let identity_file_path = identity_file.as_ref().to_string_lossy();
        info!(
            target: LOG_TARGET,
            "Restoring node identity from seed phrase at {}...",
            identity_file_path
        );
        match create_new_keypair_from_seed_words(&identity_file, words, passphrase) {
            Ok(id) => {
                info!(
                    target: LOG_TARGET,
                    "Node identity successfully restored from seed with public key: {}",
                    id.public_key()
                );
                return Ok(id);
            },
            Err(e) => {
                return Err(ExitError::new(
                    ExitCode::ConfigError,
                    format!("Failed to restore node identity from seed phrase: {e}"),
                ));
            },
        }
    }

    match load_keypair(&identity_file) {
        Ok(id) => Ok(id),
        Err(IdentityError::InvalidPermissions) => Err(ExitError::new(
            ExitCode::ConfigError,
            format!(
                "{path} has incorrect permissions. You can update the identity file with the correct permissions \
                 using 'chmod 600 {path}', or delete the identity file and a new one will be created on next start",
                path = identity_file.as_ref().to_string_lossy()
            ),
        )),
        Err(e) => {
            if create_id {
                let identity_file_path = identity_file.as_ref().to_string_lossy();
                if matches!(e, IdentityError::NotFound) {
                    debug!(
                        target: LOG_TARGET,
                        "Node identity file not found at {}. Creating new ID from seed phrase",
                        identity_file_path
                    );
                } else {
                    warn!(
                        target: LOG_TARGET,
                        "Existing node identity file at {} is invalid ({}). Creating new ID from seed phrase",
                        identity_file_path,
                        e
                    );
                }
            } else {
                let prompt = prompt("Node identity does not exist.\nWould you like to to create one (Y/n)?");
                if !prompt {
                    error!(
                        target: LOG_TARGET,
                        "Node identity not found. {}. You can update the configuration file to point to a valid node \
                         identity file, or re-run the node and create a new one.",
                        e
                    );
                    return Err(ExitError::new(
                        ExitCode::ConfigError,
                        format!(
                            "Node identity information not found. {}. You can update the configuration file to point \
                             to a valid node identity file, or re-run the node to create a new one",
                            e
                        ),
                    ));
                };
            }
            debug!(target: LOG_TARGET, "Existing node id not found. {}. Creating new ID from seed phrase", e);

            match create_new_keypair_with_seed_words(&identity_file) {
                Ok((id, words)) => {
                    info!(
                        target: LOG_TARGET,
                        "New node identity with public key {} has been created at {}.",
                        id.public_key(),
                        identity_file.as_ref().to_str().unwrap_or("?"),
                    );
                    info!(
                        target: LOG_TARGET,
                        "================================================================================"
                    );
                    info!(
                        target: LOG_TARGET,
                        "Validator Node Recovery Seed Phrase (SAVE THIS SAFELY!):"
                    );
                    info!(
                        target: LOG_TARGET,
                        "{}",
                        words.join(" ").reveal()
                    );
                    info!(
                        target: LOG_TARGET,
                        "================================================================================"
                    );
                    Ok(id)
                },
                Err(e) => {
                    error!(target: LOG_TARGET, "Could not create new node id. {}.", e);
                    Err(ExitError::new(
                        ExitCode::ConfigError,
                        format!("Could not create new node id. {}.", e),
                    ))
                },
            }
        },
    }
}

/// Tries to construct a node identity by loading the secret key and other metadata from disk and calculating the
/// missing fields from that information.
///
/// ## Parameters
/// `path` - Reference to a path
///
/// ## Returns
/// Result containing a NodeIdentity on success, string indicates the reason on failure
fn load_keypair<P: AsRef<Path>>(path: P) -> Result<RistrettoKeypair, IdentityError> {
    check_identity_file(&path)?;

    let mut file = fs::File::open(path)?;
    let id = serde_json5::from_reader::<_, RistrettoKeypair>(&mut file)?;
    debug!("Node ID loaded with public key {}", id.public_key());
    Ok(id)
}

/// Create a new node id and save it to disk
///
/// ## Parameters
/// `path` - Reference to path to save the file
/// `public_addr` - Network address of the base node
/// `peer_features` - The features enabled for the base node
///
/// ## Returns
/// Result containing the node identity, string will indicate reason on error
pub fn create_new_keypair<P: AsRef<Path>>(path: P) -> Result<RistrettoKeypair, IdentityError> {
    let node_identity = RistrettoKeypair::random(&mut rand::rng());
    save_as_json(&path, &node_identity)?;
    Ok(node_identity)
}

/// Create a new node identity derived from a CipherSeed and save it to disk.
pub fn create_new_keypair_from_seed<P: AsRef<Path>>(
    path: P,
    seed: &CipherSeed,
) -> Result<RistrettoKeypair, IdentityError> {
    let node_identity = RistrettoKeypair::derive_from_seed(seed, VN_IDENTITY_BRANCH, 0);
    save_as_json(&path, &node_identity)?;
    Ok(node_identity)
}

/// Create a new node identity derived from 24 seed words and save it to disk.
pub fn create_new_keypair_from_seed_words<P: AsRef<Path>>(
    path: P,
    seed_words: &SeedWords,
    passphrase: Option<SafePassword>,
) -> Result<RistrettoKeypair, IdentityError> {
    let seed = CipherSeed::from_mnemonic(seed_words, passphrase)?;
    create_new_keypair_from_seed(path, &seed)
}

/// Generate a fresh random seed, derive the validator node identity keypair,
/// save the identity to disk, and return `(RistrettoKeypair, SeedWords)`.
pub fn create_new_keypair_with_seed_words<P: AsRef<Path>>(
    path: P,
) -> Result<(RistrettoKeypair, SeedWords), IdentityError> {
    let seed = CipherSeed::random();
    let seed_words = seed.to_mnemonic(MnemonicLanguage::English, None)?;
    let keypair = create_new_keypair_from_seed(path, &seed)?;
    Ok((keypair, seed_words))
}

/// Derives the fee claim keypair from a CipherSeed.
pub fn derive_fee_claim_keypair_from_seed(seed: &CipherSeed, index: u64) -> RistrettoKeypair {
    RistrettoKeypair::derive_from_seed(seed, VN_FEE_CLAIM_BRANCH, index)
}

/// Derives the fee claim keypair from 24 seed words and an optional passphrase.
pub fn derive_fee_claim_keypair_from_seed_words(
    seed_words: &SeedWords,
    passphrase: Option<SafePassword>,
    index: u64,
) -> Result<RistrettoKeypair, IdentityError> {
    let seed = CipherSeed::from_mnemonic(seed_words, passphrase)?;
    Ok(derive_fee_claim_keypair_from_seed(&seed, index))
}

/// Loads the node identity from json at the given path
///
/// ## Parameters
/// `path` - Path to file from which to load the node identity
///
/// ## Returns
/// Result containing an object on success, string will indicate reason on error
pub fn load_from_json<P: AsRef<Path>, T: DeserializeOwned>(path: P) -> Result<Option<T>, IdentityError> {
    if !path.as_ref().exists() {
        return Ok(None);
    }

    let mut file = fs::File::open(path)?;
    let object = serde_json5::from_reader(&mut file)?;
    Ok(Some(object))
}

/// Saves the identity as json at a given path with restricted file permissions, creating it if it does not
/// already exist.
///
/// **Security Note:** On Unix-like systems, this sets file permissions to 0600 (read/write for owner only).
/// On Windows, no permission restrictions are applied - see module-level documentation for security implications.
///
/// ## Parameters
/// `path` - Path to save the file
/// `object` - Data to be saved
///
/// ## Returns
/// Result to check if successful or not, string will indicate reason on error
pub fn save_as_json<P: AsRef<Path>, T: Serialize>(path: P, object: &T) -> Result<(), IdentityError> {
    if let Some(p) = path.as_ref().parent() &&
        !p.exists()
    {
        fs::create_dir_all(p)?;
    }
    let json = serde_json5::to_string(object)?;
    let json_with_comment = format!(
        "// This file is generated by the Minotari base node. Any changes will be overwritten.\n{}",
        json
    );
    fs::write(path.as_ref(), json_with_comment.as_bytes())?;
    set_permissions(path, REQUIRED_IDENTITY_PERMS)?;
    Ok(())
}

/// Check that the given path exists, is a file and has the correct file permissions.
///
/// **Security Note:** Permission checks are only enforced on Unix-like systems. On Windows, this function
/// verifies file existence and type but does not check permissions. See module-level documentation.
fn check_identity_file<P: AsRef<Path>>(path: P) -> Result<(), IdentityError> {
    if !path.as_ref().exists() {
        return Err(IdentityError::NotFound);
    }

    if !path.as_ref().metadata()?.is_file() {
        return Err(IdentityError::NotFile);
    }

    if !has_permissions(&path, REQUIRED_IDENTITY_PERMS)? {
        return Err(IdentityError::InvalidPermissions);
    }
    Ok(())
}

/// Sets file permissions on Unix-like systems (Linux, macOS).
///
/// This function sets the file mode to the specified permissions value (e.g., 0600 for owner read/write only).
///
/// # Security
/// This is a critical security function that protects private keys from unauthorized access on Unix systems.
#[cfg(target_family = "unix")]
fn set_permissions<P: AsRef<Path>>(path: P, new_perms: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(&path)?;
    let mut perms = metadata.permissions();
    perms.set_mode(new_perms);
    fs::set_permissions(path, perms)?;
    Ok(())
}

/// Windows stub for setting file permissions.
///
/// # ⚠️ SECURITY WARNING ⚠️
///
/// This function does **NOT** apply any permission restrictions on Windows systems. Identity files
/// containing private keys will be created with default Windows permissions, which may allow access
/// by other users on the system.
///
/// **This is a known security limitation.** Windows users should manually restrict access to identity
/// files using Windows File Explorer or the `icacls` command. See module-level documentation for
/// detailed instructions.
///
/// # Why Not Implemented?
///
/// Windows uses Access Control Lists (ACLs) which are significantly more complex than Unix permissions.
/// Proper implementation would require:
/// - Using Windows Security APIs (via winapi or windows-sys crate)
/// - Setting DACLs (Discretionary Access Control Lists)
/// - Handling inheritance and propagation correctly
/// - Different approaches for different Windows versions
///
/// This has not been implemented due to complexity and the need for extensive Windows-specific testing.
#[cfg(target_family = "windows")]
fn set_permissions<P: AsRef<Path>>(_path: P, _new_perms: u32) -> io::Result<()> {
    // IMPORTANT: No permissions are set on Windows. See function documentation above.
    warn!(
        target: LOG_TARGET,
        "Identity file permissions are not enforced on Windows. Please manually restrict access to this file."
    );
    Ok(())
}

/// Checks if a file has the specified Unix permissions.
///
/// Returns `true` if the file's permission mode exactly matches the specified value.
#[cfg(target_family = "unix")]
fn has_permissions<P: AsRef<Path>>(path: P, perms: u32) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)?;
    Ok(metadata.permissions().mode() == perms)
}

/// Windows stub for checking file permissions.
///
/// # ⚠️ SECURITY WARNING ⚠️
///
/// This function always returns `true` on Windows, effectively disabling permission checks.
/// This means that identity files with incorrect or overly permissive access rights will **not**
/// be detected or rejected on Windows systems.
///
/// See the `set_permissions` function documentation and module-level documentation for more information
/// about this security limitation.
#[cfg(target_family = "windows")]
fn has_permissions<P: AsRef<Path>>(_path: P, _perms: u32) -> io::Result<bool> {
    // Always return true on Windows - permissions are not checked
    Ok(true)
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("Identity file has invalid permissions")]
    InvalidPermissions,
    #[error("Identity file was not found")]
    NotFound,
    #[error("Path is not a file")]
    NotFile,
    #[error("Malformed identity file: {0}")]
    JsonError(#[from] serde_json5::Error),
    #[error("Cipher seed error: {0}")]
    CipherError(#[from] CipherError),
    #[error("Mnemonic error: {0}")]
    MnemonicError(#[from] MnemonicError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_derivation_from_seed() {
        let seed = CipherSeed::random();
        let keypair1 = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0);
        let keypair2 = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0);
        assert_eq!(keypair1.public_key(), keypair2.public_key());
        assert_eq!(keypair1.secret_key(), keypair2.secret_key());

        let fee_claim_keypair = derive_fee_claim_keypair_from_seed(&seed, 0);
        // Fee claim key and validator node identity key must be domain-separated and distinct
        assert_ne!(keypair1.public_key(), fee_claim_keypair.public_key());
        assert_ne!(keypair1.secret_key(), fee_claim_keypair.secret_key());
    }

    #[test]
    fn deterministic_derivation_from_seed_words() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = dir.path().join("identity.json");

        let (keypair1, seed_words) = create_new_keypair_with_seed_words(&id_file).unwrap();
        assert_eq!(seed_words.len(), 24);

        // Derive again directly from seed words
        let keypair2 = RistrettoKeypair::derive_from_seed_words(&seed_words, None, VN_IDENTITY_BRANCH, 0).unwrap();
        assert_eq!(keypair1.public_key(), keypair2.public_key());
        assert_eq!(keypair1.secret_key(), keypair2.secret_key());

        // Restore to another file path
        let restore_file = dir.path().join("restored_identity.json");
        let restored_keypair = create_new_keypair_from_seed_words(&restore_file, &seed_words, None).unwrap();
        assert_eq!(keypair1.public_key(), restored_keypair.public_key());

        let loaded = load_keypair(&restore_file).unwrap();
        assert_eq!(keypair1.public_key(), loaded.public_key());
    }

    #[test]
    fn fee_claim_keypair_reproducible_from_seed_words() {
        let seed = CipherSeed::random();
        let words = seed.to_mnemonic(MnemonicLanguage::English, None).unwrap();

        let fee_key1 = derive_fee_claim_keypair_from_seed(&seed, 0);
        let fee_key2 = derive_fee_claim_keypair_from_seed_words(&words, None, 0).unwrap();
        assert_eq!(fee_key1.public_key(), fee_key2.public_key());
        assert_eq!(fee_key1.secret_key(), fee_key2.secret_key());
    }
}
