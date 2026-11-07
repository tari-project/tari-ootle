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

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

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
    tari_utilities::SafePassword,
};
#[cfg(feature = "p2p")]
use tari_ootle_p2p::PeerAddress;
use tari_ootle_wallet_crypto::derive_ristretto_key;

const REQUIRED_IDENTITY_PERMS: u32 = 0o100600;
const LOG_TARGET: &str = "tari::application::identity";

/// Key derivation branch for Tari validator node identity keypairs.
pub const VN_IDENTITY_BRANCH: &[u8] = b"validator_node";

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
                        "Node identity file not found at {}. Creating new ID",
                        identity_file_path
                    );
                } else {
                    warn!(
                        target: LOG_TARGET,
                        "Existing node identity file at {} is invalid ({}). Creating new ID",
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
            debug!(target: LOG_TARGET, "Existing node id not found. {}. Creating new ID", e);

            match create_new_keypair(&identity_file) {
                Ok(id) => {
                    info!(
                        target: LOG_TARGET,
                        "New node identity with public key {} has been created at {}.",
                        id.public_key(),
                        identity_file.as_ref().to_str().unwrap_or("?"),
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

/// Loads, restores or generates a validator identity. Replacing an existing validator identity requires `overwrite`.
/// Recovery words are stored in a separate owner-readable file so ordinary logs contain only the backup path.
pub fn setup_validator_keypair<P: AsRef<Path>>(
    identity_file: P,
    seed_words: Option<&SeedWords>,
    passphrase: Option<SafePassword>,
    create_id: bool,
    overwrite: bool,
) -> Result<RistrettoKeypair, ExitError> {
    setup_validator_keypair_inner(identity_file.as_ref(), seed_words, passphrase, create_id, overwrite).map_err(|e| {
        ExitError::new(
            ExitCode::ConfigError,
            format!("Could not set up validator identity: {e}"),
        )
    })
}

fn setup_validator_keypair_inner(
    identity_file: &Path,
    seed_words: Option<&SeedWords>,
    passphrase: Option<SafePassword>,
    create_id: bool,
    overwrite: bool,
) -> Result<RistrettoKeypair, IdentityError> {
    if seed_words.is_none() && passphrase.is_some() {
        return Err(IdentityError::PassphraseRequiresSeedWords);
    }
    let restored = seed_words
        .map(|words| {
            CipherSeed::from_mnemonic(words, passphrase)
                .map(|seed| RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0))
        })
        .transpose()?;

    if !overwrite {
        match load_keypair(identity_file) {
            Ok(existing) => {
                if let Some(restored) = restored &&
                    existing.public_key() != restored.public_key()
                {
                    return Err(IdentityError::IdentityExists);
                }
                return Ok(existing);
            },
            Err(IdentityError::NotFound) => {},
            Err(e) => return Err(e),
        }
    }

    let identity = match restored {
        Some(identity) => identity,
        None => {
            if !create_id && !prompt("Node identity does not exist.\nWould you like to create one (Y/n)?") {
                return Err(IdentityError::NotFound);
            }
            let seed = CipherSeed::random();
            let words = seed.to_mnemonic(MnemonicLanguage::English, None)?;
            let identity = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0);
            let recovery_path = recovery_seed_path(identity_file, identity.public_key());
            let mut file = private_tempfile(&recovery_path)?;
            file.write_all(words.join(" ").reveal().as_bytes())?;
            file.write_all(b"\n")?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&recovery_path).map_err(|e| e.error)?;
            info!(target: LOG_TARGET, "Back up the validator recovery phrase in {} to a safe location", recovery_path.display());
            identity
        },
    };
    save_identity(identity_file, &identity, overwrite)?;
    Ok(identity)
}

fn recovery_seed_path(identity_file: &Path, public_key: &RistrettoPublicKey) -> PathBuf {
    let mut name = identity_file.as_os_str().to_os_string();
    name.push(format!(".recovery-{public_key}.txt"));
    PathBuf::from(name)
}

fn private_tempfile(path: &Path) -> Result<tempfile::NamedTempFile, IdentityError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let file = tempfile::NamedTempFile::new_in(parent)?;
    set_permissions(file.path(), REQUIRED_IDENTITY_PERMS)?;
    Ok(file)
}

/// The replacement is fully serialized and synced before the destination changes. Creating an identity
/// without replacement authorization must fail if another writer creates the destination first.
fn save_identity(path: &Path, identity: &RistrettoKeypair, overwrite: bool) -> Result<(), IdentityError> {
    let json = serde_json5::to_string(identity)?;
    let mut file = private_tempfile(path)?;
    file.write_all(json.as_bytes())?;
    file.as_file().sync_all()?;
    if overwrite {
        file.persist(path).map_err(|e| e.error)?;
    } else {
        file.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
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
    #[error("A seed passphrase requires seed words to restore")]
    PassphraseRequiresSeedWords,
    #[error("A different identity already exists; use generate-identity --overwrite to replace it")]
    IdentityExists,
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

    fn create_new_keypair_from_seed(path: &Path, seed: &CipherSeed) -> Result<RistrettoKeypair, IdentityError> {
        let node_identity = RistrettoKeypair::derive_from_seed(seed, VN_IDENTITY_BRANCH, 0);
        save_identity(path, &node_identity, false)?;
        Ok(node_identity)
    }

    fn create_new_keypair_from_seed_words(
        path: &Path,
        seed_words: &SeedWords,
        passphrase: Option<SafePassword>,
    ) -> Result<RistrettoKeypair, IdentityError> {
        let seed = CipherSeed::from_mnemonic(seed_words, passphrase)?;
        create_new_keypair_from_seed(path, &seed)
    }

    fn create_new_keypair_with_seed_words(path: &Path) -> Result<(RistrettoKeypair, SeedWords), IdentityError> {
        let seed = CipherSeed::random();
        let seed_words = seed.to_mnemonic(MnemonicLanguage::English, None)?;
        let keypair = create_new_keypair_from_seed(path, &seed)?;
        Ok((keypair, seed_words))
    }

    #[test]
    fn deterministic_derivation_from_seed() {
        let seed = CipherSeed::random();
        let keypair1 = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0);
        let keypair2 = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0);
        assert_eq!(keypair1.public_key(), keypair2.public_key());
        assert_eq!(keypair1.secret_key(), keypair2.secret_key());
        let wallet_account = RistrettoKeypair::derive_from_seed(&seed, b"account", 0);
        assert_ne!(keypair1.public_key(), wallet_account.public_key());
        let next_index = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 1);
        assert_ne!(keypair1.public_key(), next_index.public_key());
    }

    #[test]
    fn deterministic_derivation_from_seed_words() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = dir.path().join("identity.json");

        let (keypair1, seed_words) = create_new_keypair_with_seed_words(&id_file).unwrap();
        assert_eq!(seed_words.len(), 24);

        let keypair2 = RistrettoKeypair::derive_from_seed_words(&seed_words, None, VN_IDENTITY_BRANCH, 0).unwrap();
        assert_eq!(keypair1.public_key(), keypair2.public_key());
        assert_eq!(keypair1.secret_key(), keypair2.secret_key());

        let restore_file = dir.path().join("restored_identity.json");
        let restored_keypair = create_new_keypair_from_seed_words(&restore_file, &seed_words, None).unwrap();
        assert_eq!(keypair1.public_key(), restored_keypair.public_key());

        let loaded = load_keypair(&restore_file).unwrap();
        assert_eq!(keypair1.public_key(), loaded.public_key());
    }

    #[test]
    fn generated_recovery_words_are_private_and_never_logged() {
        use std::sync::Mutex;

        struct CaptureLog(Mutex<Vec<String>>);
        impl log::Log for CaptureLog {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }

            fn log(&self, record: &log::Record<'_>) {
                self.0.lock().unwrap().push(record.args().to_string());
            }

            fn flush(&self) {}
        }
        static LOG: CaptureLog = CaptureLog(Mutex::new(Vec::new()));
        log::set_logger(&LOG).unwrap();
        log::set_max_level(log::LevelFilter::Info);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        let identity = setup_validator_keypair(&path, None, None, true, false).unwrap();
        let recovery_path = recovery_seed_path(&path, identity.public_key());
        let phrase = fs::read_to_string(&recovery_path).unwrap();
        let words: SeedWords = phrase.trim().parse().unwrap();
        assert_eq!(words.len(), 24);
        let restored = RistrettoKeypair::derive_from_seed_words(&words, None, VN_IDENTITY_BRANCH, 0).unwrap();
        assert_eq!(identity.public_key(), restored.public_key());
        #[cfg(unix)]
        {
            assert!(has_permissions(&path, REQUIRED_IDENTITY_PERMS).unwrap());
            assert!(has_permissions(&recovery_path, REQUIRED_IDENTITY_PERMS).unwrap());
        }
        let logs = LOG.0.lock().unwrap();
        assert!(
            logs.iter()
                .any(|line| line.contains(&recovery_path.display().to_string()))
        );
        assert!(logs.iter().all(|line| !line.contains(phrase.trim())));
    }

    #[test]
    fn restoring_a_different_seed_requires_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        let existing = create_new_keypair(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let seed = CipherSeed::random();
        let words = seed.to_mnemonic(MnemonicLanguage::English, None).unwrap();
        assert!(setup_validator_keypair(&path, Some(&words), None, true, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(load_keypair(&path).unwrap().public_key(), existing.public_key());

        let replaced = setup_validator_keypair(&path, Some(&words), None, true, true).unwrap();
        let expected = RistrettoKeypair::derive_from_seed(&seed, VN_IDENTITY_BRANCH, 0);
        assert_eq!(replaced.public_key(), expected.public_key());
        assert_eq!(load_keypair(&path).unwrap().public_key(), expected.public_key());
    }

    #[test]
    fn invalid_recovery_preserves_the_existing_identity_even_with_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        create_new_keypair(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let bad_words: SeedWords = "not a valid recovery phrase".parse().unwrap();
        assert!(setup_validator_keypair(&path, Some(&bad_words), None, true, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);

        let words = CipherSeed::random()
            .to_mnemonic(MnemonicLanguage::English, Some(SafePassword::from("correct")))
            .unwrap();
        assert!(setup_validator_keypair(&path, Some(&words), Some(SafePassword::from("wrong")), true, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn matching_seed_and_normal_restart_leave_identity_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        let (identity, words) = create_new_keypair_with_seed_words(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let with_words = setup_validator_keypair(&path, Some(&words), None, false, false).unwrap();
        let without_words = setup_validator_keypair(&path, None, None, false, false).unwrap();
        assert_eq!(with_words.public_key(), identity.public_key());
        assert_eq!(without_words.public_key(), identity.public_key());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn low_level_creation_does_not_clobber_an_existing_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        create_new_keypair(&path).unwrap();
        let original = fs::read(&path).unwrap();
        assert!(create_new_keypair_from_seed(&path, &CipherSeed::random()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_passphrase_without_words_does_not_create_an_unencrypted_seed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        assert!(setup_validator_keypair(&path, None, Some(SafePassword::from("secret")), true, false).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn failed_replacement_preserves_the_destination_and_removes_the_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("destination");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("keep"), b"original").unwrap();
        let identity = RistrettoKeypair::random(&mut rand::rng());
        assert!(save_identity(&path, &identity, true).is_err());
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"original");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
