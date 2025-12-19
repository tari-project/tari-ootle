//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::consts::{U32, U64};
use tari_crypto::hash_domain;
use tari_hashing::DomainSeparatedBorshHasher;
use tari_ootle_common_types::Network;

hash_domain!(OotleWalletHashDomain, "com.tari.ootle.wallet", 1);
pub type OotleWalletHasher32<M> = DomainSeparatedBorshHasher<M, Blake2b<U32>>;
pub type OotleWalletHasher64<M> = DomainSeparatedBorshHasher<M, Blake2b<U64>>;
pub(crate) fn wallet_hasher64(network: Network, label: &'static str) -> OotleWalletHasher64<OotleWalletHashDomain> {
    OotleWalletHasher64::new_with_label(&format!("{}.n{}", label, network.as_byte()))
}

/// Used to derive the owner secret key for stealth outputs
pub fn stealth_owner_hasher64(network: Network) -> OotleWalletHasher64<OotleWalletHashDomain> {
    wallet_hasher64(network, "stealth_owner")
}

pub fn stealth_output_tag_hasher64(network: Network) -> OotleWalletHasher64<OotleWalletHashDomain> {
    wallet_hasher64(network, "output_tag")
}
