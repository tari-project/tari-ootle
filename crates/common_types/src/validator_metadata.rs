//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_hashing::TransactionHashDomain;
use tari_template_lib_types::{
    Hash32,
    crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes},
};

use crate::{Network, SubstateAddress, base_layer_hashing::TariBaseLayerHasher32};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidatorMetadata {
    pub public_key: RistrettoPublicKeyBytes,
    pub vn_shard_key: SubstateAddress,
    pub signature: SchnorrSignatureBytes,
}

impl ValidatorMetadata {
    pub fn new(
        public_key: RistrettoPublicKeyBytes,
        vn_shard_key: SubstateAddress,
        signature: SchnorrSignatureBytes,
    ) -> Self {
        Self {
            public_key,
            vn_shard_key,
            signature,
        }
    }
}

pub fn vn_node_hash(
    network: Network,
    public_key: &RistrettoPublicKeyBytes,
    substate_address: &SubstateAddress,
) -> Hash32 {
    let arr: [u8; Hash32::LENGTH] = TariBaseLayerHasher32::<TransactionHashDomain>::new_with_label(&format!(
        "validator_nodes.n{}",
        network.as_byte()
    ))
    .chain(public_key)
    .chain(substate_address.array())
    .finalize()
    .into();
    Hash32::from(arr)
}
