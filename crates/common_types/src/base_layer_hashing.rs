//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use blake2::Blake2b;
use digest::consts::{U32, U64};
use ootle_network::Network;
use tari_crypto::hashing::DomainSeparatedHasher;
use tari_hashing::{ConfidentialOutputHashDomain, DomainSeparatedBorshHasher, WalletOutputEncryptionKeysDomain};

pub type TariBaseLayerHasher64<M> = DomainSeparatedBorshHasher<M, Blake2b<U64>>;
pub type TariBaseLayerHasher32<M> = DomainSeparatedBorshHasher<M, Blake2b<U32>>;
fn confidential_hasher64(network: Network, label: &'static str) -> TariBaseLayerHasher64<ConfidentialOutputHashDomain> {
    TariBaseLayerHasher64::new_with_label(&format!("{}.n{}", label, network.as_byte()))
}

pub type WalletOutputEncryptionKeysDomainHasher = DomainSeparatedHasher<Blake2b<U64>, WalletOutputEncryptionKeysDomain>;

/// Hasher for encrypting wallet output data.
pub fn encrypted_data_hasher() -> WalletOutputEncryptionKeysDomainHasher {
    // This is identical to the base layer hasher to allow burn outputs to be decrypted.
    // TODO: this isn't strictly necessary, separate hashers could be used for L1/L2 which *hand wavy* could be more
    // secure.
    WalletOutputEncryptionKeysDomainHasher::new_with_label("")
}

pub fn ownership_proof_hasher64(network: Network) -> TariBaseLayerHasher64<ConfidentialOutputHashDomain> {
    confidential_hasher64(network, "commitment_signature")
}
