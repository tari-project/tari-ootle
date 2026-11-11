//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_ootle_common_types::NodeHeight;
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct OnForceBeat {
    receiver: watch::Receiver<Option<NodeHeight>>,
    sender: Arc<watch::Sender<Option<NodeHeight>>>,
}

impl OnForceBeat {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(None);
        Self {
            receiver,
            sender: Arc::new(sender),
        }
    }

    pub async fn wait(&mut self) -> Option<NodeHeight> {
        self.receiver.changed().await.expect("sender can never be dropped");
        let mut height = None;
        self.sender.send_if_modified(|current| {
            height = current.take();
            false
        });
        height
    }

    pub fn beat(&self, new_height: Option<NodeHeight>) {
        self.sender.send_modify(|current| {
            *current = (*current).max(new_height);
        });
    }
}

impl Default for OnForceBeat {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_timer_beat_does_not_clobber_an_unconsumed_forced_height() {
        let mut force_beat = OnForceBeat::new();
        force_beat.beat(Some(NodeHeight(10)));
        force_beat.beat(None);

        assert_eq!(force_beat.wait().await, Some(NodeHeight(10)));
    }

    #[tokio::test]
    async fn a_consumed_forced_height_is_cleared_for_the_next_timer_beat() {
        let mut force_beat = OnForceBeat::new();
        force_beat.beat(Some(NodeHeight(4)));
        force_beat.beat(Some(NodeHeight(9)));
        assert_eq!(force_beat.wait().await, Some(NodeHeight(9)));

        force_beat.beat(None);
        assert_eq!(force_beat.wait().await, None);
    }
}
