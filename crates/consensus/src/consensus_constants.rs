//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{env, time::Duration};

use tari_ootle_common_types::NumPreshards;
use tari_ootle_transaction::Network;

#[derive(Clone, Debug)]
pub struct ConsensusConstants {
    /// Number of base layer confirmations required before an L1 block is considered unable to re-org.
    pub base_layer_confirmations: u64,
    /// The target size of the committee per shard group.
    pub committee_size_per_shard_group: u32,
    /// The number of preshards to break up the shard space.
    pub num_preshards: NumPreshards,
    /// The maximum block time. The pacemaker will trigger a new view if a block is not received within this time +
    /// delta.
    pub pacemaker_block_time: Duration,
    /// The number of missed proposals before a node will immediately send a NEWVIEW to the next leader when the node
    /// who missed the proposals is selected as leader.
    pub missed_proposal_suspend_threshold: u64,
    /// The number of missed proposals before a EvictNode command is proposed.
    pub missed_proposal_evict_threshold: u64,
    /// The number of rounds a node must participate before their non-participation is reset. If a peer is offline,
    /// gets suspended and comes online, their missed proposal count (up to a maximum of
    /// `missed_proposal_recovery_threshold`) is decremented for each block that they participate (vote) in. Once
    /// this reaches zero, the node is considered stable and out of suspension.
    pub missed_proposal_recovery_threshold: u64,
    /// The maximum number of commands that a block may contain.
    pub max_number_commands_in_block: usize,
    /// The value that fees are divided by to determine the amount of fees to burn. 0 means no fees are burned.
    pub fee_exhaust_divisor: u64,
    /// Number of base-layer blocks of leeway a voter is allowed when accepting `EndEpoch` proposals.
    /// If the voter's oracle has not yet crossed the next epoch boundary but its lagged scan height
    /// is within this many blocks of the boundary, the voter accepts `EndEpoch` from peers whose
    /// oracle has already crossed. Must be uniform network-wide to avoid divergent voting.
    /// Set to 0 to disable leeway.
    pub epoch_end_spread_blocks: u64,
}

impl ConsensusConstants {
    pub const fn mainnet() -> Self {
        Self {
            base_layer_confirmations: 1000,
            committee_size_per_shard_group: 40,
            num_preshards: NumPreshards::current(),
            pacemaker_block_time: Duration::from_secs(10),
            missed_proposal_suspend_threshold: 5,
            missed_proposal_evict_threshold: 10,
            missed_proposal_recovery_threshold: 5,
            max_number_commands_in_block: 100,
            fee_exhaust_divisor: 20, // 1/20 = 5%
            epoch_end_spread_blocks: 10,
        }
    }

    pub const fn devnet(committee_size: u32) -> Self {
        Self {
            base_layer_confirmations: 3,
            committee_size_per_shard_group: committee_size,
            num_preshards: NumPreshards::current(),
            pacemaker_block_time: Duration::from_secs(10),
            missed_proposal_suspend_threshold: 5,
            missed_proposal_evict_threshold: 10,
            missed_proposal_recovery_threshold: 5,
            max_number_commands_in_block: 100,
            fee_exhaust_divisor: 20, // 1/20 = 5%
            epoch_end_spread_blocks: 1,
        }
    }

    pub const fn esmeralda() -> Self {
        Self {
            base_layer_confirmations: 100,
            committee_size_per_shard_group: 40,
            num_preshards: NumPreshards::current(),
            pacemaker_block_time: Duration::from_secs(10),
            missed_proposal_suspend_threshold: 5,
            missed_proposal_evict_threshold: 10,
            missed_proposal_recovery_threshold: 5,
            max_number_commands_in_block: 100,
            fee_exhaust_divisor: 20, // 1/20 = 5%
            epoch_end_spread_blocks: 5,
        }
    }

    pub const fn testnet() -> Self {
        Self {
            base_layer_confirmations: 100,
            committee_size_per_shard_group: 40,
            num_preshards: NumPreshards::current(),
            pacemaker_block_time: Duration::from_secs(10),
            missed_proposal_suspend_threshold: 5,
            missed_proposal_evict_threshold: 10,
            missed_proposal_recovery_threshold: 5,
            max_number_commands_in_block: 100,
            fee_exhaust_divisor: 20, // 1/20 = 5%
            epoch_end_spread_blocks: 5,
        }
    }
}

impl From<Network> for ConsensusConstants {
    fn from(network: Network) -> Self {
        match network {
            Network::MainNet => Self::mainnet(),
            // Allow committee size to be overridden for LocalNet
            Network::LocalNet => Self::devnet(
                env::var("TARI_DEVNET_COMMITTEE_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(7),
            ),
            Network::Esmeralda => Self::esmeralda(),
            Network::StageNet | Network::NextNet | Network::Igor => Self::testnet(),
        }
    }
}
