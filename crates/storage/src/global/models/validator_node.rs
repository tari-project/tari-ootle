//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_ootle_common_types::{
    displayable::Displayable,
    vn_node_hash,
    Epoch,
    NodeAddressable,
    SubstateAddress,
    VotePower,
};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorNode<TAddr> {
    pub address: TAddr,
    pub public_key: RistrettoPublicKeyBytes,
    pub shard_key: SubstateAddress,
    pub start_epoch: Epoch,
    pub end_epoch: Option<Epoch>,
    pub fee_claim_public_key: RistrettoPublicKeyBytes,
    pub vote_power: VotePower,
}

impl<TAddr: NodeAddressable> ValidatorNode<TAddr> {
    pub fn get_node_hash(&self, network: Network) -> FixedHash {
        vn_node_hash(network, &self.public_key, &self.shard_key)
    }
}

impl<TAddr> PartialEq for ValidatorNode<TAddr> {
    fn eq(&self, other: &Self) -> bool {
        self.shard_key == other.shard_key
    }
}

impl<TAddr> Eq for ValidatorNode<TAddr> {}

impl<TAddr> std::hash::Hash for ValidatorNode<TAddr> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.shard_key.hash(state);
    }
}

impl<TAddr: NodeAddressable> Display for ValidatorNode<TAddr> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Validator(pk={}, shard_key={}, addr={}, start_epoch={}, end_epoch={})",
            self.public_key,
            self.shard_key,
            self.address,
            self.start_epoch,
            self.end_epoch.display()
        )
    }
}
