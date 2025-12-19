//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_ootle_common_types::NodeHeight;
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, Default)]
pub struct LeaderTimeout {
    pub current_height: NodeHeight,
    pub current_high_pc: NodeHeight,
    pub num_timeouts: u32,
}

impl LeaderTimeout {
    pub fn delta(&self) -> u64 {
        self.current_height
            .as_u64()
            .saturating_sub(self.current_high_pc.as_u64())
    }
}

#[derive(Debug, Clone)]
pub struct OnLeaderTimeout {
    // todo: consider using a different sync construct, like an mpsc channel
    receiver: watch::Receiver<LeaderTimeout>,
    sender: Arc<watch::Sender<LeaderTimeout>>,
}

impl OnLeaderTimeout {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(LeaderTimeout::default());
        Self {
            receiver,
            sender: Arc::new(sender),
        }
    }

    pub async fn wait(&mut self) -> LeaderTimeout {
        self.receiver.changed().await.expect("sender can never be dropped");
        // This could lead to a more recent value being seen. Idk if that is ok...
        *self.receiver.borrow()
    }

    pub fn leader_timed_out(&self, timeout: LeaderTimeout) {
        self.sender.send(timeout).expect("receiver can never be dropped")
    }
}

impl Default for OnLeaderTimeout {
    fn default() -> Self {
        Self::new()
    }
}
