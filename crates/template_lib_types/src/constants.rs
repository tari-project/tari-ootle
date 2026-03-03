//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A collection of convenient constant values

use crate::{
    ObjectKey,
    substates::{ComponentAddress, ResourceAddress, VaultId},
};
// TODO: These addresses are set pretty arbitrarily.

/// Resource address for all public identity-based non-fungible tokens.
/// This resource provides a space for a virtual token representing ownership based on a public key.
/// resource_0100000000000000000000000000000000000000000000000000000000000000
pub const PUBLIC_IDENTITY_RESOURCE_ADDRESS: ResourceAddress = ResourceAddress::new(ObjectKey::from_array([
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]));

/// The Tari network native resource address. This token is used for paying network fees, among other things. It is a
/// fungible resource with a divisibility of 6, meaning that the smallest unit is 0.000001 TARI.
/// resource_0101010101010101010101010101010101010101010101010101010101010101
pub const STEALTH_TARI_RESOURCE_ADDRESS: ResourceAddress =
    ResourceAddress::new(ObjectKey::from_array([1u8; ObjectKey::LENGTH]));

/// Shorthand version of the `STEALTH_TARI_RESOURCE_ADDRESS` constant
pub const TARI_TOKEN: ResourceAddress = STEALTH_TARI_RESOURCE_ADDRESS;
#[deprecated(since = "0.24.5", note = "Use TARI_TOKEN instead")]
pub const XTR: ResourceAddress = STEALTH_TARI_RESOURCE_ADDRESS;
/// One XTR in the smallest divisible units i.e. 1 TARI = 1,000,000 micro TARI
/// For example: 10 * TARI = 10 TARI = 10,000,000 micro TARI
pub const TARI: u64 = 1_000_000;
#[deprecated(since = "0.24.5", note = "Use TARI instead")]
pub const ONE_XTR: u64 = TARI;

/// Address of testnet faucet component
pub const XTR_FAUCET_COMPONENT_ADDRESS: ComponentAddress = ComponentAddress::new(ObjectKey::from_array([
    1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]));

/// Address of the faucet vault
pub const XTR_FAUCET_VAULT_ADDRESS: VaultId = VaultId::new(ObjectKey::from_array([
    1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]));

/// Address of the NFT faucet component
pub const NFT_FAUCET_COMPONENT_ADDRESS: ComponentAddress = ComponentAddress::new(ObjectKey::from_array([
    0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]));
/// Address of the builtin NFT faucet resource
/// resource_ff00000000000000000000000000000000000000000000000000000000000001
pub const NFT_FAUCET_RESOURCE_ADDRESS: ResourceAddress = ResourceAddress::new(ObjectKey::from_array([
    0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]));

/// Metadata key used as convention to represent the symbol (a.k.a. ticker) of a token. Meant as a shorthand,
/// user-friendly identification of the underlying token
pub const TOKEN_SYMBOL: &str = "SYMBOL";
/// Metadata key used as convention to represent the image URL of a token. Meant to be used in user interfaces
/// to display the token's logo or image
pub const IMAGE_URL: &str = "IMAGE_URL";
/// Default divisibility for fungible resources (8)
pub const DEFAULT_DIVISIBILITY: u8 = 8;
