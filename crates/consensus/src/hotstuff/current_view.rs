//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_ootle_common_types::{Epoch, NodeHeight};
use tokio::sync::watch;

/// The (epoch, height) view of the pacemaker. Views are ordered by epoch, then height, because heights restart from
/// zero each epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct View {
    pub epoch: Epoch,
    pub height: NodeHeight,
}

impl View {
    pub const fn new(epoch: Epoch, height: NodeHeight) -> Self {
        Self { epoch, height }
    }
}

impl Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.epoch, self.height)
    }
}

/// A read-only view of the pacemaker's current view, for readers outside the consensus worker. The worker owns the
/// pacemaker and is the only writer.
#[derive(Debug, Clone)]
pub struct CurrentView {
    receiver: watch::Receiver<View>,
}

impl CurrentView {
    pub(crate) fn new(receiver: watch::Receiver<View>) -> Self {
        Self { receiver }
    }

    /// A view that stays at epoch 0, height 0, for consensus handles with no running worker.
    pub fn detached() -> Self {
        let (_, receiver) = watch::channel(View::default());
        Self { receiver }
    }

    pub fn get(&self) -> View {
        *self.receiver.borrow()
    }

    pub fn get_epoch(&self) -> Epoch {
        self.get().epoch
    }

    pub fn get_height(&self) -> NodeHeight {
        self.get().height
    }
}

impl Display for CurrentView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}
