//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Fresh Ootle identity minting: the account keypair (owner secret + public) and the view keypair
//! (stealth-receive).
//!
//! Two flavours:
//!
//! - `generate_*` — the production path. Draws the secret scalar from `OsRng` (the only RNG site here; never on the
//!   seed path). The resulting keypair is fresh and non-reproducible.
//! - `derive_*_from_seed` — the deterministic path. Calls no RNG; it derives the secret scalar from a caller-supplied
//!   32-byte seed via the canonical wallet KDF ([`tari_ootle_wallet_crypto::derive_ristretto_key`]) with the wallet's
//!   two distinct branch labels (`"account"` / `"view_only_key"`). Two runs with the same seed produce byte-identical
//!   keys, and the account key is independent of the view key (different branch labels), so a single seed yields a
//!   complete, wallet-compatible identity.
//!
//! The secret half is a [`SecretKeyBytes`] (zeroized on drop); the public half is a [`PublicKeyBytes`].

use rand::RngExt as _;
use tari_crypto::{keys::SecretKey as _, ristretto::RistrettoSecretKey, tari_utilities::ByteArray as _};
use tari_engine_types::component::derive_component_address_from_public_key;
use tari_ootle_wallet_crypto::derive_ristretto_key;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;

use crate::{
    keys::public_key_bytes_from_secret,
    types::{
        address::ComponentAddressStr,
        bytes::{PublicKeyBytes, SecretKeyBytes},
        error::OotleSdkError,
    },
};

/// The wallet `KeyBranch` label the account key is derived under (matches the wallet's HD key
/// manager). Domain-separates the account key from the view key for the same seed.
const ACCOUNT_BRANCH: &[u8] = b"account";

/// The wallet `KeyBranch` label the view-only key is derived under. Distinct from [`ACCOUNT_BRANCH`]
/// so account and view keys never collapse to the same scalar for a given seed.
const VIEW_BRANCH: &[u8] = b"view_only_key";

/// A freshly-minted Ootle keypair: the secret scalar and its derived public key.
///
/// The secret is a [`SecretKeyBytes`] — no `Copy`, wiped on drop; the public key is a
/// [`PublicKeyBytes`]. The public key is always `public_key_bytes_from_secret(secret)`.
///
/// Deliberately **not** `Clone`: cloning would silently duplicate the secret material (defeating the
/// `no-Copy` guarantee on [`SecretKeyBytes`]) and create two independently-dropped copies. Mint a fresh
/// keypair (or move this one) instead of cloning it.
#[derive(Debug)]
pub struct OotleKeypair {
    /// The secret scalar (32-byte canonical Ristretto scalar). Zeroized on drop.
    pub secret: SecretKeyBytes,
    /// The public key (`G * secret`, 32 bytes).
    pub public_key: PublicKeyBytes,
}

impl OotleKeypair {
    /// Wraps an internal [`RistrettoSecretKey`] into the boundary keypair, deriving the public key.
    fn from_internal_secret(secret: &RistrettoSecretKey) -> Self {
        let public_internal = public_key_bytes_from_secret(secret);
        // RistrettoSecretKey is always 32 canonical bytes, so this never errors.
        let secret_bytes =
            SecretKeyBytes::from_bytes(secret.as_bytes()).expect("RistrettoSecretKey is always 32 bytes");
        OotleKeypair {
            secret: secret_bytes,
            public_key: PublicKeyBytes::from_internal(&public_internal),
        }
    }
}

/// Draws a fresh canonical Ristretto secret scalar from `OsRng`.
///
/// Draws 64 uniform bytes and reduces them via [`RistrettoSecretKey::from_uniform_bytes`], which always
/// yields a canonical scalar (drawing raw 32 bytes could occasionally land on a non-canonical encoding).
/// This is the only RNG site in this module.
fn random_secret() -> RistrettoSecretKey {
    let mut rng = rand::rng();
    let wide = rng.random::<[u8; 64]>();
    RistrettoSecretKey::from_uniform_bytes(&wide).expect("64 uniform bytes reduce to a canonical scalar")
}

/// Mints a fresh **account** keypair from `OsRng` (production). Non-reproducible by design.
pub fn generate_account_keypair() -> OotleKeypair {
    OotleKeypair::from_internal_secret(&random_secret())
}

/// Mints a fresh **view** keypair from `OsRng` (production). Non-reproducible by design.
pub fn generate_view_keypair() -> OotleKeypair {
    OotleKeypair::from_internal_secret(&random_secret())
}

/// Deterministically derives the **account** keypair from a 32-byte seed (no RNG; reproducible
/// byte-for-byte).
///
/// Uses the canonical wallet KDF ([`derive_ristretto_key`]) under the `"account"` branch label at
/// index `0`. Reproducible byte-for-byte and independent of [`derive_view_keypair_from_seed`].
pub fn derive_account_keypair_from_seed(seed: &[u8; 32]) -> Result<OotleKeypair, OotleSdkError> {
    Ok(OotleKeypair::from_internal_secret(&derive_ristretto_key(
        seed,
        ACCOUNT_BRANCH,
        0,
    )))
}

/// Deterministically derives the **view** keypair from a 32-byte seed (no RNG; reproducible
/// byte-for-byte).
///
/// Uses the canonical wallet KDF ([`derive_ristretto_key`]) under the `"view_only_key"` branch label
/// at index `0`. Reproducible byte-for-byte and independent of [`derive_account_keypair_from_seed`].
pub fn derive_view_keypair_from_seed(seed: &[u8; 32]) -> Result<OotleKeypair, OotleSdkError> {
    Ok(OotleKeypair::from_internal_secret(&derive_ristretto_key(
        seed,
        VIEW_BRANCH,
        0,
    )))
}

/// Derives the canonical account component address from an account public key.
///
/// This is the domain-separated Blake2b-256 hash the engine uses to place an account:
/// `H(ComponentAddress-domain ‖ ACCOUNT_TEMPLATE_ADDRESS ‖ public_key)` (the `public_key` is
/// Borsh-length-prefixed by `.chain()`). It calls the engine entry point
/// [`derive_component_address_from_public_key`] under the builtin [`ACCOUNT_TEMPLATE_ADDRESS`]. A
/// wrong derivation would send funds to an address nobody controls, so the hash is never
/// re-implemented here.
///
/// Takes only the public key — no `network` parameter: the engine derivation is network-independent
/// (template + pk). The result is the canonical `component_<hex>` string.
///
/// Infallible at the engine level (it hashes 32 bytes and does not validate that the key is a
/// canonical curve point); the [`Result`] exists for symmetry with the rest of the boundary and to
/// leave room for a future point-validation error path. The public-key bytes are accepted as-is.
pub fn derive_account_address(account_public_key: &PublicKeyBytes) -> Result<ComponentAddressStr, OotleSdkError> {
    let component =
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &account_public_key.to_internal());
    Ok(ComponentAddressStr::from_internal(&component))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    /// The public key always equals `G * secret` (the canonical derivation) for all four entry points.
    #[test]
    fn public_matches_derive_for_all_entry_points() {
        for kp in [
            generate_account_keypair(),
            generate_view_keypair(),
            derive_account_keypair_from_seed(&seed(1)).unwrap(),
            derive_view_keypair_from_seed(&seed(1)).unwrap(),
        ] {
            let sk = crate::keys::parse_secret_key(&kp.secret).expect("minted secret is canonical");
            let expected = PublicKeyBytes::from_internal(&public_key_bytes_from_secret(&sk));
            assert_eq!(kp.public_key, expected, "public key must be G * secret");
        }
    }

    /// The seed path is deterministic: the same seed reproduces the same keypair byte-for-byte.
    #[test]
    fn seed_path_is_reproducible() {
        let a = derive_account_keypair_from_seed(&seed(7)).unwrap();
        let b = derive_account_keypair_from_seed(&seed(7)).unwrap();
        assert_eq!(a.secret, b.secret);
        assert_eq!(a.public_key, b.public_key);

        let v1 = derive_view_keypair_from_seed(&seed(7)).unwrap();
        let v2 = derive_view_keypair_from_seed(&seed(7)).unwrap();
        assert_eq!(v1.secret, v2.secret);
        assert_eq!(v1.public_key, v2.public_key);
    }

    /// Account and view keys are domain-separated: the same seed yields distinct keypairs.
    #[test]
    fn account_and_view_differ_for_the_same_seed() {
        let acct = derive_account_keypair_from_seed(&seed(42)).unwrap();
        let view = derive_view_keypair_from_seed(&seed(42)).unwrap();
        assert_ne!(acct.secret, view.secret, "account != view (distinct branch labels)");
        assert_ne!(acct.public_key, view.public_key);
    }

    /// Distinct seeds yield distinct account keys.
    #[test]
    fn distinct_seeds_yield_distinct_keys() {
        let a = derive_account_keypair_from_seed(&seed(1)).unwrap();
        let b = derive_account_keypair_from_seed(&seed(2)).unwrap();
        assert_ne!(a.secret, b.secret);
        assert_ne!(a.public_key, b.public_key);
    }

    /// Two production calls draw independent secrets (overwhelmingly different).
    #[test]
    fn production_calls_differ() {
        let a = generate_account_keypair();
        let b = generate_account_keypair();
        assert_ne!(a.secret, b.secret, "two OsRng draws must differ");
    }

    /// `derive_account_address` reproduces the engine derivation used by the transfer builder
    /// byte-for-byte: same template, same pk, same domain-separated hash. This is the lost-funds
    /// guard — the public fn must never diverge from the engine call.
    #[test]
    fn derive_account_address_matches_engine_path() {
        // A minted public key (deterministic, so the assertion is over a real on-curve key).
        let kp = derive_account_keypair_from_seed(&seed(9)).unwrap();
        let pk = kp.public_key;

        let derived = derive_account_address(&pk).unwrap();

        // The internal engine call the builder uses, reproduced directly here.
        let expected = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &pk.to_internal());
        let expected = ComponentAddressStr::from_internal(&expected);

        assert_eq!(derived, expected, "public derivation must equal the engine derivation");
        assert!(derived.as_str().starts_with("component_"), "canonical component_<hex>");
    }

    /// Distinct public keys derive distinct account addresses.
    #[test]
    fn distinct_keys_yield_distinct_addresses() {
        let a = derive_account_address(&derive_account_keypair_from_seed(&seed(1)).unwrap().public_key).unwrap();
        let b = derive_account_address(&derive_account_keypair_from_seed(&seed(2)).unwrap().public_key).unwrap();
        assert_ne!(a, b);
    }
}
