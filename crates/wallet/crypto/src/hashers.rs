//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use borsh::BorshSerialize;
use chacha20poly1305::aead::generic_array::GenericArray;
use digest::{
    Digest,
    consts::{U32, U64},
};
use ootle_network::Network;
use tari_crypto::{
    hash_domain,
    hashing::{DomainSeparatedHasher, DomainSeparation},
};
use tari_hashing::DomainSeparatedBorshHasher;

use crate::kdfs::SafeKey64;

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

pub trait KdfHasher<T: ?Sized> {
    type HashOutput;
    fn kdf_digest(self, data: &T) -> Self::HashOutput;
}

impl<M: DomainSeparation, T: BorshSerialize + ?Sized> KdfHasher<T> for OotleWalletHasher64<M> {
    type HashOutput = SafeKey64;

    fn kdf_digest(mut self, data: &T) -> Self::HashOutput {
        self.update_consensus_encode(data);
        let mut out = SafeKey64::default();
        self.finalize_into(GenericArray::from_mut_slice(out.as_mut()));
        out
    }
}

// impl<M: DomainSeparation> KdfHasher<[u8]> for DomainSeparatedHasher<Blake2b<U32>, M> {
//     type HashOutput = DomainSeparatedHash<Blake2b<U32>>;
//
//     fn kdf_digest(self, data: &[u8]) -> Self::HashOutput {
//         self.digest(data)
//     }
// }

impl<M: DomainSeparation> KdfHasher<[u8]> for DomainSeparatedHasher<Blake2b<U64>, M> {
    type HashOutput = SafeKey64;

    fn kdf_digest(mut self, data: &[u8]) -> Self::HashOutput {
        let mut out = SafeKey64::default();
        self.update(data);
        Digest::finalize_into(self, GenericArray::from_mut_slice(out.as_mut()));
        out
    }
}
