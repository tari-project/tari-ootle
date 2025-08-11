//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::consts::{U32, U64};
use tari_crypto::hashing::DomainSeparatedHasher;
use tari_hashing::{ConfidentialOutputHashDomain, DomainSeparatedBorshHasher, WalletOutputEncryptionKeysDomain};

use crate::Network;

pub type TariBaseLayerHasher64<M> = DomainSeparatedBorshHasher<M, Blake2b<U64>>;
pub type TariBaseLayerHasher32<M> = DomainSeparatedBorshHasher<M, Blake2b<U32>>;
fn confidential_hasher64(network: Network, label: &'static str) -> TariBaseLayerHasher64<ConfidentialOutputHashDomain> {
    TariBaseLayerHasher64::new_with_label(&format!("{}.n{}", label, network.as_byte()))
}

type WalletOutputEncryptionKeysDomainHasher = DomainSeparatedHasher<Blake2b<U64>, WalletOutputEncryptionKeysDomain>;

pub fn encrypted_data_hasher() -> WalletOutputEncryptionKeysDomainHasher {
    WalletOutputEncryptionKeysDomainHasher::new_with_label("")
}

pub fn ownership_proof_hasher64(network: Network) -> TariBaseLayerHasher64<ConfidentialOutputHashDomain> {
    confidential_hasher64(network, "commitment_signature")
}
