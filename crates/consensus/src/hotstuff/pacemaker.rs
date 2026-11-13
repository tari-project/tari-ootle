//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{cmp, pin::Pin, time::Duration};

use log::*;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tokio::{
    sync::watch,
    time::{Instant, Sleep},
};

use crate::hotstuff::{
    current_view::{CurrentView, View},
    pacemaker_action::{PacemakerAction, PacemakerActions},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::pacemaker";
const MAX_DELTA: Duration = Duration::from_secs(300);
/// Propose a block OFFSET_BLOCK_TIME sooner than the required block time
const OFFSET_BLOCK_TIME: Duration = Duration::from_secs(1);
/// Fixed network latency allowance added to every leader timeout deadline.
const AVG_LATENCY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
pub(crate) enum PacemakerEvent {
    /// Propose if this node is the leader and there is something to propose.
    Beat,
    /// Propose if this node is the leader, even if the block is empty. `forced_height` is set when a timeout
    /// certificate requires a proposal at that height; it is `None` when the block timer elapsed.
    ForceBeat { forced_height: Option<NodeHeight> },
    /// No proposal for the current view arrived in time. `num_timeouts` counts the consecutive timeouts since the view
    /// last advanced.
    LeaderTimeout { num_timeouts: u32 },
}

/// The HotStuff pacemaker: the current view, the block timer and the leader timeout. It is owned by the consensus
/// worker, which polls [`PaceMaker::next_event`] in its event loop, so every state change is applied before the next
/// timer is observed.
#[derive(Debug)]
pub(crate) struct PaceMaker {
    view: View,
    published_view: watch::Sender<View>,
    high_pc_height: NodeHeight,
    block_time: Duration,
    is_started: bool,
    num_timeouts: u32,
    block_timer: Pin<Box<Sleep>>,
    leader_timeout: Pin<Box<Sleep>>,
    is_beat_pending: bool,
    /// A forced beat requested by a handler, returned before any timer event. Timer-forced beats are returned as soon
    /// as the block timer elapses, so they never overwrite this.
    pending_forced_height: Option<NodeHeight>,
}

impl PaceMaker {
    pub fn new(max_base_time: Duration) -> Self {
        let (published_view, _) = watch::channel(View::default());
        Self {
            view: View::default(),
            published_view,
            high_pc_height: NodeHeight::zero(),
            block_time: max_base_time,
            is_started: false,
            num_timeouts: 0,
            block_timer: Box::pin(tokio::time::sleep_until(far_future())),
            leader_timeout: Box::pin(tokio::time::sleep_until(far_future())),
            is_beat_pending: false,
            pending_forced_height: None,
        }
    }

    pub fn current_view(&self) -> View {
        self.view
    }

    pub fn current_epoch(&self) -> Epoch {
        self.view.epoch
    }

    pub fn current_height(&self) -> NodeHeight {
        self.view.height
    }

    /// A read-only view that follows this pacemaker's view.
    pub fn subscribe_view(&self) -> CurrentView {
        CurrentView::new(self.published_view.subscribe())
    }

    /// Waits for the next pacemaker event. Cancel safe: no state changes until the event is returned.
    pub async fn next_event(&mut self) -> PacemakerEvent {
        if let Some(forced_height) = self.pending_forced_height.take() {
            return PacemakerEvent::ForceBeat {
                forced_height: Some(forced_height),
            };
        }
        // Elapsed timers go before a requested beat, so a steady stream of beats cannot hold them back. The leader
        // timeout goes first: the block timer is zero-length when the block time is at most OFFSET_BLOCK_TIME, so it
        // can be elapsed on every call. The block timer counts as elapsed once the timer driver has fired it, which
        // paces a zero-length block timer at the driver's resolution.
        if self.leader_timeout.deadline() <= Instant::now() {
            return self.on_leader_timeout_elapsed();
        }
        if self.block_timer.is_elapsed() {
            return self.on_block_timer_elapsed();
        }
        if self.is_beat_pending {
            self.is_beat_pending = false;
            return PacemakerEvent::Beat;
        }

        tokio::select! {
            biased;
            () = &mut self.leader_timeout => self.on_leader_timeout_elapsed(),
            () = &mut self.block_timer => self.on_block_timer_elapsed(),
        }
    }

    fn on_block_timer_elapsed(&mut self) -> PacemakerEvent {
        self.reset_block_timer();
        PacemakerEvent::ForceBeat { forced_height: None }
    }

    fn on_leader_timeout_elapsed(&mut self) -> PacemakerEvent {
        self.reset_block_timer();
        self.reset_leader_timeout_timer();
        self.num_timeouts = self.num_timeouts.saturating_add(1);
        info!(target: LOG_TARGET, "⚠️ Leader timeout! Current view: {}, Delta: {:.2?}", self.view, self.delta_time());
        PacemakerEvent::LeaderTimeout {
            num_timeouts: self.num_timeouts,
        }
    }

    /// Starts the timers and beats, if not already started. The view is entered either way.
    pub fn start(&mut self, epoch: Epoch, height: NodeHeight, high_pc_height: NodeHeight) {
        self.enter(View::new(epoch, height));
        info!(target: LOG_TARGET, "🚀 Starting pacemaker at leaf height {} and high QC: {}", self.view, high_pc_height);
        if self.is_started {
            return;
        }
        self.num_timeouts = 0;
        self.high_pc_height = high_pc_height;
        info!(target: LOG_TARGET, "Reset! Current height: {}, Delta: {:.2?}", self.view, self.delta_time());
        self.reset_leader_timeout_timer();
        self.reset_block_timer();
        self.is_beat_pending = true;
        self.is_started = true;
    }

    /// Stops the timers. Requested beats are kept.
    pub fn stop(&mut self) {
        info!(target: LOG_TARGET, "💤 Stopping pacemaker");
        self.is_started = false;
        self.num_timeouts = 0;
        self.block_timer.as_mut().reset(far_future());
        self.leader_timeout.as_mut().reset(far_future());
    }

    pub fn beat(&mut self) {
        self.is_beat_pending = true;
    }

    /// Applies the actions a handler requested, in the order it requested them.
    pub fn apply(&mut self, actions: &mut PacemakerActions) {
        for action in actions.drain() {
            match action {
                PacemakerAction::Beat => self.beat(),
                PacemakerAction::ForceBeat { height } => self.force_beat(height),
                PacemakerAction::EnterView {
                    epoch,
                    height,
                    high_pc_height,
                } => self.enter_view(epoch, height, high_pc_height),
                PacemakerAction::ResetLeaderTimeout { high_pc_height } => self.reset(high_pc_height, false),
                PacemakerAction::SetEpoch { epoch } => self.set_epoch(epoch),
            }
        }
    }

    fn force_beat(&mut self, forced_height: NodeHeight) {
        self.pending_forced_height = Some(forced_height);
    }

    /// Enters `(epoch, height)` and restarts both timers if it is later than the current view, otherwise does
    /// nothing.
    fn enter_view(&mut self, epoch: Epoch, height: NodeHeight, high_pc_height: NodeHeight) {
        if self.enter(View::new(epoch, height)) {
            self.reset(high_pc_height, true);
        }
    }

    /// Moves to height zero of `epoch` and restarts both timers. Called once the end of epoch block is committed.
    fn set_epoch(&mut self, epoch: Epoch) {
        self.set_view(View::new(epoch, NodeHeight::zero()));
        info!(target: LOG_TARGET, "🧿 PACEMAKER: reset View updated to {}", self.view);
        self.reset(NodeHeight::zero(), true);
    }

    fn reset(&mut self, high_pc_height: NodeHeight, reset_block_timer: bool) {
        if !self.is_started {
            return;
        }
        self.num_timeouts = 0;
        self.high_pc_height = high_pc_height;
        self.reset_leader_timeout_timer();
        if reset_block_timer {
            self.reset_block_timer();
            info!(target: LOG_TARGET, "🧿 Pacemaker Reset! Current height: {}, Delta: {:.2?}", self.view, self.delta_time());
        } else {
            info!(target: LOG_TARGET, "🧿 Pacemaker Leader timeout Reset! Current height: {}, Delta: {:.2?}", self.view, self.delta_time());
        }
    }

    /// Moves to `view` if it is later than the current view.
    fn enter(&mut self, view: View) -> bool {
        if view <= self.view {
            return false;
        }
        info!(target: LOG_TARGET, "🧿 PACEMAKER: View updated from {} to {}", self.view, view);
        self.set_view(view);
        true
    }

    fn set_view(&mut self, view: View) {
        self.view = view;
        self.published_view.send_replace(view);
    }

    fn reset_block_timer(&mut self) {
        let deadline = Instant::now() + self.block_time.saturating_sub(OFFSET_BLOCK_TIME);
        self.block_timer.as_mut().reset(deadline);
    }

    /// Lower bound on the interval between two [`PacemakerEvent::LeaderTimeout`] events at `block_time`.
    pub const fn min_leader_timeout_interval(block_time: Duration) -> Duration {
        block_time.saturating_add(AVG_LATENCY)
    }

    /// The leader timeout is block_time + delta + latency, so it always elapses after the block timer.
    fn reset_leader_timeout_timer(&mut self) {
        // TODO: get real avg latency
        let deadline = Instant::now() + self.block_time + self.delta_time() + AVG_LATENCY;
        self.leader_timeout.as_mut().reset(deadline);
    }

    /// Delta time is defined as 2^n where n is the difference in height between the current view height and the high
    /// QC height.
    fn delta_time(&self) -> Duration {
        let current_height = self.view.height;
        if current_height.is_zero() || self.high_pc_height.is_zero() {
            // Allow extra time for the first block
            return self.block_time * 2;
        }
        let exp = u32::try_from(cmp::min(
            u64::from(u32::MAX),
            cmp::max(1, current_height.saturating_sub(self.high_pc_height).as_u64()),
        ))
        .unwrap_or(u32::MAX);
        cmp::min(
            MAX_DELTA,
            2u64.checked_pow(exp).map(Duration::from_secs).unwrap_or(MAX_DELTA),
        )
    }
}

fn far_future() -> Instant {
    // Taken verbatim from the tokio library:
    // Roughly 30 years from now.
    // API does not provide a way to obtain max `Instant`
    // or convert specific date in the future to instant.
    // 1000 years overflows on macOS, 100 years overflows on FreeBSD.
    Instant::now() + Duration::from_secs(86400 * 365 * 30)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The event that is ready without time passing, if any.
    async fn ready_event(pacemaker: &mut PaceMaker) -> Option<PacemakerEvent> {
        tokio::time::timeout(Duration::ZERO, pacemaker.next_event()).await.ok()
    }

    async fn started(block_time: Duration) -> PaceMaker {
        let mut pacemaker = PaceMaker::new(block_time);
        pacemaker.start(Epoch(1), NodeHeight(1), NodeHeight(1));
        assert!(matches!(ready_event(&mut pacemaker).await, Some(PacemakerEvent::Beat)));
        pacemaker
    }

    /// The timeout count of the next leader timeout, skipping the block timer's forced beats.
    async fn next_leader_timeout(pacemaker: &mut PaceMaker) -> u32 {
        loop {
            if let PacemakerEvent::LeaderTimeout { num_timeouts } = pacemaker.next_event().await {
                return num_timeouts;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_duplicate_start_keeps_the_running_timeout_count() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        assert_eq!(next_leader_timeout(&mut pacemaker).await, 1);

        pacemaker.start(Epoch(1), NodeHeight(1), NodeHeight(1));
        assert_eq!(next_leader_timeout(&mut pacemaker).await, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_and_restart_resets_the_timeout_count() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        assert_eq!(next_leader_timeout(&mut pacemaker).await, 1);

        pacemaker.stop();
        pacemaker.start(Epoch(1), NodeHeight(2), NodeHeight(2));
        assert_eq!(next_leader_timeout(&mut pacemaker).await, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stopped_pacemaker_never_times_out() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        pacemaker.stop();

        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(ready_event(&mut pacemaker).await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn an_elapsed_block_timer_does_not_replace_a_forced_height() {
        let mut pacemaker = started(Duration::from_secs(5)).await;
        pacemaker.force_beat(NodeHeight(10));
        tokio::time::advance(Duration::from_secs(5)).await;

        assert!(matches!(pacemaker.next_event().await, PacemakerEvent::ForceBeat {
            forced_height: Some(NodeHeight(10))
        }));
        assert!(matches!(pacemaker.next_event().await, PacemakerEvent::ForceBeat {
            forced_height: None
        }));
    }

    #[tokio::test(start_paused = true)]
    async fn an_elapsed_leader_timeout_goes_before_a_requested_beat() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        tokio::time::advance(Duration::from_secs(3600)).await;
        pacemaker.beat();

        assert!(matches!(
            ready_event(&mut pacemaker).await,
            Some(PacemakerEvent::LeaderTimeout { num_timeouts: 1 })
        ));
        assert!(matches!(ready_event(&mut pacemaker).await, Some(PacemakerEvent::Beat)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_zero_length_block_timer_does_not_hold_back_the_leader_timeout() {
        let mut pacemaker = PaceMaker::new(OFFSET_BLOCK_TIME);
        pacemaker.start(Epoch(1), NodeHeight(1), NodeHeight(1));
        tokio::time::advance(Duration::from_secs(3600)).await;

        assert!(matches!(
            ready_event(&mut pacemaker).await,
            Some(PacemakerEvent::LeaderTimeout { num_timeouts: 1 })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn entering_a_view_after_the_deadline_cancels_the_leader_timeout() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        tokio::time::advance(Duration::from_secs(3600)).await;

        pacemaker.enter_view(Epoch(1), NodeHeight(2), NodeHeight(1));
        assert!(ready_event(&mut pacemaker).await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn the_leader_timeout_fires_after_the_deadline_when_the_view_does_not_advance() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        tokio::time::advance(Duration::from_secs(3600)).await;

        pacemaker.enter_view(Epoch(1), NodeHeight(1), NodeHeight(1));
        assert_eq!(next_leader_timeout(&mut pacemaker).await, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn actions_apply_in_the_order_they_were_requested() {
        let mut pacemaker = started(Duration::from_secs(2)).await;
        let mut actions = PacemakerActions::new();
        actions.set_epoch(Epoch(2));
        actions.enter_view(Epoch(2), NodeHeight(3), NodeHeight(2));
        actions.beat();

        pacemaker.apply(&mut actions);

        assert_eq!(pacemaker.current_view(), View::new(Epoch(2), NodeHeight(3)));
        assert!(matches!(ready_event(&mut pacemaker).await, Some(PacemakerEvent::Beat)));
        assert!(actions.drain().next().is_none());
    }

    #[tokio::test]
    async fn it_enters_a_later_epoch_at_a_lower_height() {
        let mut pacemaker = PaceMaker::new(Duration::from_secs(1));
        let view = pacemaker.subscribe_view();
        assert!(pacemaker.enter(View::new(Epoch(8601), NodeHeight(11378))));

        assert!(pacemaker.enter(View::new(Epoch(8602), NodeHeight(0))));
        assert_eq!(view.get(), View::new(Epoch(8602), NodeHeight(0)));
    }

    #[tokio::test]
    async fn it_only_moves_forward_within_an_epoch() {
        let mut pacemaker = PaceMaker::new(Duration::from_secs(1));
        assert!(pacemaker.enter(View::new(Epoch(1), NodeHeight(10))));

        assert!(!pacemaker.enter(View::new(Epoch(1), NodeHeight(10))));
        assert!(!pacemaker.enter(View::new(Epoch(1), NodeHeight(9))));
        assert_eq!(pacemaker.current_height(), NodeHeight(10));

        assert!(pacemaker.enter(View::new(Epoch(1), NodeHeight(11))));
        assert_eq!(pacemaker.current_height(), NodeHeight(11));
    }

    #[tokio::test]
    async fn it_never_enters_an_earlier_view() {
        let mut pacemaker = PaceMaker::new(Duration::from_secs(1));
        assert!(pacemaker.enter(View::new(Epoch(2), NodeHeight(5))));

        assert!(!pacemaker.enter(View::new(Epoch(1), NodeHeight(100))));
        assert_eq!(pacemaker.current_view(), View::new(Epoch(2), NodeHeight(5)));
    }
}
