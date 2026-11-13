//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, SubstateAddress};
use tari_sidechain::FixedHash;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tari_transaction_components::{tari_amount::MicroMinotari, transaction_components::TransactionOutput};

#[derive(Debug, Clone)]
pub struct BaseLayerMetadata {
    pub height_of_longest_chain: u64,
    pub tip_hash: FixedHash,
}

#[derive(Debug, Clone)]
pub struct SideChainUtxos {
    pub block_info: BlockInfo,
    pub outputs: Vec<TransactionOutput>,
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub hash: FixedHash,
    pub height: u64,
    pub next_block_hash: Option<FixedHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "base-node-client/"))]
pub struct BaseLayerValidatorNode {
    pub public_key: RistrettoPublicKeyBytes,
    pub shard_key: SubstateAddress,
    pub sidechain_id: Option<RistrettoPublicKeyBytes>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLayerConsensusConstants {
    pub epoch_length: u64,
    pub validator_node_registration_min_deposit_amount: MicroMinotari,
}

impl BaseLayerConsensusConstants {
    pub fn height_to_epoch(&self, height: u64) -> Epoch {
        Epoch(height / self.epoch_length.max(1))
    }

    pub fn epoch_to_height(&self, epoch: Epoch) -> u64 {
        epoch.0.saturating_mul(self.epoch_length.max(1))
    }

    pub fn validator_node_registration_min_deposit_amount(&self) -> MicroMinotari {
        self.validator_node_registration_min_deposit_amount
    }

    pub fn epoch_length(&self) -> u64 {
        self.epoch_length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_height_conversions_are_consistent_when_epoch_length_is_zero() {
        let constants = BaseLayerConsensusConstants {
            epoch_length: 0,
            validator_node_registration_min_deposit_amount: MicroMinotari(0),
        };

        for epoch_num in [0u64, 1, 7, 42] {
            let epoch = Epoch(epoch_num);
            let height = constants.epoch_to_height(epoch);
            assert_eq!(height, epoch_num);
            assert_eq!(constants.height_to_epoch(height), epoch);
        }
    }

    #[test]
    fn epoch_and_height_conversions_with_non_zero_epoch_length() {
        let constants = BaseLayerConsensusConstants {
            epoch_length: 10,
            validator_node_registration_min_deposit_amount: MicroMinotari(100),
        };

        assert_eq!(constants.epoch_to_height(Epoch(0)), 0);
        assert_eq!(constants.epoch_to_height(Epoch(3)), 30);
        assert_eq!(constants.height_to_epoch(29), Epoch(2));
        assert_eq!(constants.height_to_epoch(30), Epoch(3));
        assert_eq!(constants.epoch_to_height(Epoch(u64::MAX)), u64::MAX);
    }
}
