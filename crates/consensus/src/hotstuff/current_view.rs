//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::Display,
    sync::{Arc, atomic, atomic::AtomicU64},
};

use log::info;
use tari_ootle_common_types::{Epoch, NodeHeight};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::current_view";

#[derive(Debug, Clone, Default)]
pub struct CurrentView {
    height: Arc<AtomicU64>,
    epoch: Arc<AtomicU64>,
}

impl CurrentView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_epoch(&self) -> Epoch {
        self.epoch.load(atomic::Ordering::SeqCst).into()
    }

    pub fn get_height(&self) -> NodeHeight {
        self.height.load(atomic::Ordering::SeqCst).into()
    }

    /// Updates the view if (epoch, height) is lexicographically greater than the current view. Heights restart from
    /// zero each epoch, so entering a later epoch must always take the new height, even when it is lower than the
    /// current one.
    pub(crate) fn enter(&self, epoch: Epoch, height: NodeHeight) -> bool {
        let current_epoch = self.get_epoch();
        let current_height = self.get_height();
        let is_updated = if epoch > current_epoch {
            self.epoch.store(epoch.as_u64(), atomic::Ordering::SeqCst);
            self.height.store(height.as_u64(), atomic::Ordering::SeqCst);
            true
        } else if epoch == current_epoch && height > current_height {
            self.height.store(height.as_u64(), atomic::Ordering::SeqCst);
            true
        } else {
            false
        };

        if is_updated {
            info!(target: LOG_TARGET, "🧿 PACEMAKER: View updated from {current_epoch}/{current_height} to {self}");
        }
        is_updated
    }

    // /// Sets the epoch to epoch + 1
    // pub(crate) fn next_epoch(&self) {
    //     let epoch = self.epoch.fetch_add(1, atomic::Ordering::SeqCst);
    //     info!(target: LOG_TARGET, "🧿 PACEMAKER SET EPOCH: {}", epoch + 1);
    // }

    /// Resets the height and epoch. Prefer update.
    pub(crate) fn reset(&self, epoch: Epoch, height: NodeHeight) {
        self.epoch.store(epoch.as_u64(), atomic::Ordering::SeqCst);
        self.height.store(height.as_u64(), atomic::Ordering::SeqCst);
        info!(target: LOG_TARGET, "🧿 PACEMAKER: reset View updated to {epoch}/{height}");
    }
}

impl Display for CurrentView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.get_epoch(), self.get_height())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_enters_a_later_epoch_at_a_lower_height() {
        let view = CurrentView::new();
        assert!(view.enter(Epoch(8601), NodeHeight(11378)));

        assert!(view.enter(Epoch(8602), NodeHeight(0)));
        assert_eq!(view.get_epoch(), Epoch(8602));
        assert_eq!(view.get_height(), NodeHeight(0));
    }

    #[test]
    fn it_only_moves_forward_within_an_epoch() {
        let view = CurrentView::new();
        assert!(view.enter(Epoch(1), NodeHeight(10)));

        assert!(!view.enter(Epoch(1), NodeHeight(10)));
        assert!(!view.enter(Epoch(1), NodeHeight(9)));
        assert_eq!(view.get_height(), NodeHeight(10));

        assert!(view.enter(Epoch(1), NodeHeight(11)));
        assert_eq!(view.get_height(), NodeHeight(11));
    }

    #[test]
    fn it_never_enters_an_earlier_view() {
        let view = CurrentView::new();
        assert!(view.enter(Epoch(2), NodeHeight(5)));

        assert!(!view.enter(Epoch(1), NodeHeight(100)));
        assert_eq!(view.get_epoch(), Epoch(2));
        assert_eq!(view.get_height(), NodeHeight(5));
    }
}
