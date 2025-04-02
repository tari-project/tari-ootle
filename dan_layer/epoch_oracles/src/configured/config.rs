//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    hash::{DefaultHasher, Hasher},
    time::Duration,
};

use tari_common_types::types::PublicKey;
use tari_dan_common_types::{Epoch, NumPreshards, ShardGroup, SubstateAddress};
use tari_engine_types::serde_with;
use tari_utilities::byte_array::ByteArray;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(with = "serde_with::duration::optional_seconds")]
    pub epoch_time: Option<Duration>,
    pub initial_epoch: Epoch,
    #[serde(default)]
    pub validators: Vec<Validator>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            epoch_time: None,
            initial_epoch: Epoch(0),
            validators: vec![],
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Validator {
    pub public_key: PublicKey,
    pub claim_key: PublicKey,
    pub shard_group: ShardGroup,
    pub registration_epoch: Epoch,
}

impl Validator {
    /// Generates a deterministic shard key that naturally falls within the ShardGroup for this Validator node.
    pub(crate) fn calculate_shard_key(&self) -> SubstateAddress {
        let range = self.shard_group.to_substate_address_range(NumPreshards::current());
        let mut hasher = DefaultHasher::new();
        hasher.write(&self.shard_group.encode_as_u32().to_be_bytes());
        hasher.write(self.public_key.as_bytes());
        hasher.write(self.claim_key.as_bytes());
        hasher.write(&self.registration_epoch.to_be_bytes());
        let hash = hasher.finish();

        let start = range.start();
        let len = start.object_key_bytes().len();
        let hash_size = size_of_val(&hash);
        let mut start = start.into_array();
        start[len - hash_size..len].copy_from_slice(&hash.to_be_bytes());
        SubstateAddress::from_array(start)
    }
}
