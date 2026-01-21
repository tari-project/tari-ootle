//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A collection of convenient constant values

use tari_template_lib_types::ObjectKey;

use crate::models::{ComponentAddress, ResourceAddress, VaultId};
// TODO: These addresses are set pretty arbitrarily.

/// Resource address for all public identity-based non-fungible tokens.
/// This resource provides a space for a virtual token representing ownership based on a public key.
/// resource_0100000000000000000000000000000000000000000000000000000000000000
pub const PUBLIC_IDENTITY_RESOURCE_ADDRESS: ResourceAddress = ResourceAddress::new(ObjectKey::from_array([
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]));

/// The Tari network native resource address, used for paying network fees
/// resource_0101010101010101010101010101010101010101010101010101010101010101
pub const STEALTH_TARI_RESOURCE_ADDRESS: ResourceAddress =
    ResourceAddress::new(ObjectKey::from_array([1u8; ObjectKey::LENGTH]));

/// Shorthand version of the `STEALTH_TARI_RESOURCE_ADDRESS` constant
pub const XTR: ResourceAddress = STEALTH_TARI_RESOURCE_ADDRESS;
/// One XTR in the smallest divisible units i.e. 1 XTR = 1,000,000 micro XTR
/// For example: 10 * MXTR = 10 XTR
pub const ONE_XTR: u64 = 1_000_000;

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
