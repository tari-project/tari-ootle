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
use rand::{CryptoRng, RngCore, rngs::OsRng};
use serde::{Serialize, de::DeserializeOwned};
use tari_common::{
    configuration::bootstrap::prompt,
    exit_codes::{ExitCode, ExitError},
};
use tari_crypto::{
    keys::{PublicKey as _, SecretKey as _},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
#[cfg(feature = "p2p")]
use tari_ootle_p2p::PeerAddress;

const REQUIRED_IDENTITY_PERMS: u32 = 0o100600;
const LOG_TARGET: &str = "tari::identity";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RistrettoKeypair(Arc<KeyPairInner>);

impl RistrettoKeypair {
    pub fn random<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let secret_key = RistrettoSecretKey::random(rng);
        Self::from_secret_key(secret_key)
    }

    pub fn from_secret_key(secret_key: RistrettoSecretKey) -> Self {
        let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
        Self(Arc::new(KeyPairInner { secret_key, public_key }))
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
/// - `public_address` - Network address of the base node
/// - `create_id` - Only applies if the identity_file does not exist or is malformed. If true, a new identity will be
///   created, otherwise the user will be prompted to create a new ID
/// - `peer_features` - Enables features of the base node
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
    let node_identity = RistrettoKeypair::random(&mut OsRng);
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
    #[error("Identity file has invalid permissions")]
    InvalidPermissions,
    #[error("Identity file was not found")]
    NotFound,
    #[error("Path is not a file")]
    NotFile,
    #[error("Malformed identity file: {0}")]
    JsonError(#[from] serde_json5::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}
