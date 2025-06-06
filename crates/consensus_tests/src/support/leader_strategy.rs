//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::traits::LeaderStrategy;
use tari_ootle_common_types::{committee::Committee, displayable::Displayable, NodeAddressable, NodeHeight};

#[derive(Debug, Clone, Copy, Default)]
pub struct RoundRobinLeaderStrategy;
impl RoundRobinLeaderStrategy {
    pub fn new() -> Self {
        Self
    }
}

impl<TAddr: NodeAddressable> LeaderStrategy<TAddr> for RoundRobinLeaderStrategy {
    fn calculate_leader(&self, committee: &Committee<TAddr>, height: NodeHeight) -> u32 {
        log::info!(
            target: "tari::ootle::consensus::leader_strategy",
            "RoundRobinLeaderStrategy: calculate_leader committee: {}, height: {}",
            committee.address_iter().collect::<Vec<_>>().display(),
            height
        );
        (height.as_u64() % committee.len() as u64) as u32
    }
}
