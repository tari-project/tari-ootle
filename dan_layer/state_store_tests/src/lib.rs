//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(test)]
const TEST_NUM_PRESHARDS: tari_dan_common_types::NumPreshards = tari_dan_common_types::NumPreshards::P256;

#[cfg(test)]
mod block_diffs;
#[cfg(test)]
mod blocks;
#[cfg(test)]
mod foreign_parked_proposals;
#[cfg(test)]
mod foreign_proposals;
#[cfg(test)]
mod foreign_substate_pledges;
#[cfg(test)]
mod helpers;
#[cfg(test)]
mod misc;
#[cfg(test)]
mod missing_transactions;
#[cfg(test)]
mod quorum_certificates;
#[cfg(test)]
mod state_transitions;
#[cfg(test)]
mod state_tree;
#[cfg(test)]
mod state_tree_diff;
#[cfg(test)]
mod substate_locks;
#[cfg(test)]
mod substates;
#[cfg(test)]
mod transactions;
