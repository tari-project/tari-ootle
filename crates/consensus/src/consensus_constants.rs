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
    /// The maximum total weight of commands a leader will pack into a single block. This is a budget
    /// of transaction weight (see `Transaction::calculate_transaction_weight`) rather than a flat
    /// command count, so heavy transactions consume more of a block than light ones. This is a local
    /// proposing heuristic only — it is not validated when receiving/voting on a block, so it carries
    /// no fork risk and nodes may run different values.
    pub max_block_weight: u64,
    /// A hard upper bound on the number of commands in a block, independent of weight. Bounds the
    /// on-the-wire/`BTreeSet` overhead so a flood of near-zero-weight commands cannot bloat a block.
    /// Like `max_block_weight`, this is a propose-time heuristic and is not validated on receive.
    pub max_commands_in_block: usize,
    /// The maximum total transaction execution weight a block may contain to be considered valid. Unlike
    /// `max_block_weight` (a local proposing heuristic) this IS enforced when receiving/voting on a block:
    /// a block whose transaction execution weight exceeds this — and that contains more than one
    /// transaction command — is rejected (no-vote). It bounds how long a replica can be made to spend
    /// executing a block, preventing a misbehaving leader from pushing replicas past the block time.
    /// Set above `max_block_weight` so honest proposals are never rejected. CONSENSUS RULE: must be
    /// uniform network-wide, otherwise nodes diverge on block validity.
    pub max_block_validation_weight: u64,
    /// The maximum total WASM metering points a leader will pack into a single block, summed from each
    /// transaction's actual metered execution (`ExecuteResult::wasm_execution_points`). Transaction weight is
    /// size/IO-based and blind to execution cost, so a low-weight compute-heavy transaction evades the weight
    /// budget — this bounds the pure WASM compute a block adds on top of the weight-bounded work. Like
    /// `max_block_weight`, this is a local proposing heuristic only and carries no fork risk. A transaction's
    /// points are only known after it executes, so a block may overshoot this budget by up to
    /// `MAX_WASM_POINTS_PER_TRANSACTION`; `max_block_validation_wasm_points` must allow for this.
    pub max_block_wasm_points: u64,
    /// The maximum total WASM metering points a block may contain to be considered valid. Like
    /// `max_block_validation_weight` this IS enforced when receiving/voting on a block: a replica keeps a
    /// running points total while executing the block's commands and stops at the first command that pushes the
    /// total over this limit (no-vote), bounding the CPU a misbehaving leader can extract from replicas.
    /// Metering is deterministic, so every replica stops at the same command and votes identically. Must be at
    /// least `max_block_wasm_points + MAX_WASM_POINTS_PER_TRANSACTION` so honest proposals are never rejected.
    /// CONSENSUS RULE: must be uniform network-wide, otherwise nodes diverge on block validity.
    pub max_block_validation_wasm_points: u64,
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
            // Calibrated against 2-core hardware (Esmeralda class), where ~500 LocalOnly stress
            // transactions (~62 weight each, ~31k weight) executed in ~11.5s — i.e. ~2.7k weight/s.
            // A 10000 budget (~160 of those commands) therefore projects to ~3.7s of propose-time
            // execution, comfortably within the 10s block time and under the 5s execution circuit
            // breaker, while heavier transactions naturally consume more of the budget. The breaker in
            // on_propose is the backstop for outliers the static weight under-estimates. NOTE:
            // propose-time execution is sequential, so more cores does not raise this proportionally —
            // calibrate to single-core throughput.
            max_block_weight: 10_000,
            max_commands_in_block: 1000,
            // 1.5x the proposal budget: honest blocks (<= max_block_weight) are never rejected, while a
            // full validation-weight block projects to ~5.5s of execution on 2-core hardware — well
            // within the 10s block time. Rejects the ~31k-weight/500-command overload that broke things.
            max_block_validation_weight: 15_000,
            // ~45 max-compute transactions (100M points each, ~33ms on ~3GHz x86) — ~1.5s of serial WASM
            // execution with ~3x headroom for slower validator hardware. Provisional pending re-measurement
            // of the metering costs on x86-class hardware.
            max_block_wasm_points: 4_500_000_000,
            // Proposal budget + one max-points transaction (the post-execution overshoot) + margin, so
            // honest proposals are never rejected.
            max_block_validation_wasm_points: 5_000_000_000,
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
            // Calibrated against 2-core hardware (Esmeralda class), where ~500 LocalOnly stress
            // transactions (~62 weight each, ~31k weight) executed in ~11.5s — i.e. ~2.7k weight/s.
            // A 10000 budget (~160 of those commands) therefore projects to ~3.7s of propose-time
            // execution, comfortably within the 10s block time and under the 5s execution circuit
            // breaker, while heavier transactions naturally consume more of the budget. The breaker in
            // on_propose is the backstop for outliers the static weight under-estimates. NOTE:
            // propose-time execution is sequential, so more cores does not raise this proportionally —
            // calibrate to single-core throughput.
            max_block_weight: 10_000,
            max_commands_in_block: 1000,
            // 1.5x the proposal budget: honest blocks (<= max_block_weight) are never rejected, while a
            // full validation-weight block projects to ~5.5s of execution on 2-core hardware — well
            // within the 10s block time. Rejects the ~31k-weight/500-command overload that broke things.
            max_block_validation_weight: 15_000,
            // ~45 max-compute transactions (100M points each, ~33ms on ~3GHz x86) — ~1.5s of serial WASM
            // execution with ~3x headroom for slower validator hardware. Provisional pending re-measurement
            // of the metering costs on x86-class hardware.
            max_block_wasm_points: 4_500_000_000,
            // Proposal budget + one max-points transaction (the post-execution overshoot) + margin, so
            // honest proposals are never rejected.
            max_block_validation_wasm_points: 5_000_000_000,
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
            // Calibrated against 2-core hardware (Esmeralda class), where ~500 LocalOnly stress
            // transactions (~62 weight each, ~31k weight) executed in ~11.5s — i.e. ~2.7k weight/s.
            // A 10000 budget (~160 of those commands) therefore projects to ~3.7s of propose-time
            // execution, comfortably within the 10s block time and under the 5s execution circuit
            // breaker, while heavier transactions naturally consume more of the budget. The breaker in
            // on_propose is the backstop for outliers the static weight under-estimates. NOTE:
            // propose-time execution is sequential, so more cores does not raise this proportionally —
            // calibrate to single-core throughput.
            max_block_weight: 10_000,
            max_commands_in_block: 1000,
            // 1.5x the proposal budget: honest blocks (<= max_block_weight) are never rejected, while a
            // full validation-weight block projects to ~5.5s of execution on 2-core hardware — well
            // within the 10s block time. Rejects the ~31k-weight/500-command overload that broke things.
            max_block_validation_weight: 15_000,
            // ~45 max-compute transactions (100M points each, ~33ms on ~3GHz x86) — ~1.5s of serial WASM
            // execution with ~3x headroom for slower validator hardware. Provisional pending re-measurement
            // of the metering costs on x86-class hardware.
            max_block_wasm_points: 4_500_000_000,
            // Proposal budget + one max-points transaction (the post-execution overshoot) + margin, so
            // honest proposals are never rejected.
            max_block_validation_wasm_points: 5_000_000_000,
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
            // Calibrated against 2-core hardware (Esmeralda class), where ~500 LocalOnly stress
            // transactions (~62 weight each, ~31k weight) executed in ~11.5s — i.e. ~2.7k weight/s.
            // A 10000 budget (~160 of those commands) therefore projects to ~3.7s of propose-time
            // execution, comfortably within the 10s block time and under the 5s execution circuit
            // breaker, while heavier transactions naturally consume more of the budget. The breaker in
            // on_propose is the backstop for outliers the static weight under-estimates. NOTE:
            // propose-time execution is sequential, so more cores does not raise this proportionally —
            // calibrate to single-core throughput.
            max_block_weight: 10_000,
            max_commands_in_block: 1000,
            // 1.5x the proposal budget: honest blocks (<= max_block_weight) are never rejected, while a
            // full validation-weight block projects to ~5.5s of execution on 2-core hardware — well
            // within the 10s block time. Rejects the ~31k-weight/500-command overload that broke things.
            max_block_validation_weight: 15_000,
            // ~45 max-compute transactions (100M points each, ~33ms on ~3GHz x86) — ~1.5s of serial WASM
            // execution with ~3x headroom for slower validator hardware. Provisional pending re-measurement
            // of the metering costs on x86-class hardware.
            max_block_wasm_points: 4_500_000_000,
            // Proposal budget + one max-points transaction (the post-execution overshoot) + margin, so
            // honest proposals are never rejected.
            max_block_validation_wasm_points: 5_000_000_000,
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

#[cfg(test)]
mod tests {
    use tari_engine_types::limits::MAX_WASM_POINTS_PER_TRANSACTION;

    use super::*;

    #[test]
    fn validation_budgets_always_admit_honest_proposals() {
        for constants in [
            ConsensusConstants::mainnet(),
            ConsensusConstants::devnet(7),
            ConsensusConstants::esmeralda(),
            ConsensusConstants::testnet(),
        ] {
            assert!(constants.max_block_validation_weight >= constants.max_block_weight);
            // A leader only learns a transaction's points after executing it, so an honest block may
            // exceed the propose budget by up to one transaction's full points budget.
            assert!(
                constants.max_block_validation_wasm_points >=
                    constants.max_block_wasm_points + MAX_WASM_POINTS_PER_TRANSACTION
            );
        }
    }
}
