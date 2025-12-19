//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use tari_crypto::{hash_domain, hashing::DomainSeparation};
use tari_hashing::{Blake2bU64, DomainSeparatedBorshHasher};

hash_domain!(TariOotleTransactionHashDomainV1, "com.tari.ootle.transaction", 1);

pub fn transaction_hasher_v1(label: &'static str) -> TariTransactionHasher<TariOotleTransactionHashDomainV1> {
    TariTransactionHasher::<TariOotleTransactionHashDomainV1>::new_with_label(label)
}

pub struct TariTransactionHasher<H> {
    hasher: DomainSeparatedBorshHasher<H, Blake2bU64>,
}

impl<H: DomainSeparation> TariTransactionHasher<H> {
    pub fn new_with_label(label: &'static str) -> Self {
        let hasher = DomainSeparatedBorshHasher::new_with_label(label);
        Self { hasher }
    }

    pub fn update<T: BorshSerialize + ?Sized>(&mut self, data: &T) {
        self.hasher.update_consensus_encode(data)
    }

    pub fn chain<T: BorshSerialize + ?Sized>(mut self, data: &T) -> Self {
        self.update(data);
        self
    }

    pub fn result(self) -> [u8; 64] {
        self.hasher.finalize().into()
    }
}
