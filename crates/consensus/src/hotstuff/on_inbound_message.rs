//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{BTreeMap, VecDeque};

use log::*;
use tari_consensus_types::Vote;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::{
    hotstuff::error::HotStuffError,
    messages::HotstuffMessage,
    traits::{ConsensusSpec, InboundMessaging, hooks::ConsensusHooks},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::inbound_messages";

type IncomingMessageResult<TAddr> = Result<Option<(TAddr, HotstuffMessage)>, HotStuffError>;

pub struct OnInboundMessage<TConsensusSpec: ConsensusSpec> {
    message_buffer: MessageBuffer<TConsensusSpec>,
    hooks: TConsensusSpec::Hooks,
}

impl<TConsensusSpec: ConsensusSpec> OnInboundMessage<TConsensusSpec> {
    pub fn new(inbound_messaging: TConsensusSpec::InboundMessaging, hooks: TConsensusSpec::Hooks) -> Self {
        Self {
            message_buffer: MessageBuffer::new(inbound_messaging),
            hooks,
        }
    }

    /// Returns the next message that is ready for consensus. The future returned from this function is cancel safe, and
    /// can be used with tokio::select! macro.
    pub async fn next_message(
        &mut self,
        current_epoch: Epoch,
        current_height: NodeHeight,
        has_processed_first_block: bool,
    ) -> Option<Result<(TConsensusSpec::Addr, HotstuffMessage), HotStuffError>> {
        // Then incoming messages for the current epoch/height
        let result = self
            .message_buffer
            .next(current_epoch, current_height, has_processed_first_block)
            .await;
        match result {
            Ok(Some((from, msg))) => {
                self.hooks.on_message_received(&msg);
                Some(Ok((from, msg)))
            },
            Ok(None) => {
                // Inbound messages terminated
                None
            },
            Err(err) => Some(Err(err)),
        }
    }

    /// Discards all buffered messages including ones queued up for processing and returns when complete.
    pub async fn discard(&mut self) {
        self.message_buffer.discard().await;
    }

    pub fn clear_buffer(&mut self) {
        self.message_buffer.clear_buffer();
    }
}

type EpochAndHeight = (Epoch, NodeHeight);
pub struct MessageBuffer<TConsensusSpec: ConsensusSpec> {
    buffer: BTreeMap<EpochAndHeight, VecDeque<(TConsensusSpec::Addr, HotstuffMessage)>>,
    inbound_messaging: TConsensusSpec::InboundMessaging,
}

impl<TConsensusSpec: ConsensusSpec> MessageBuffer<TConsensusSpec> {
    pub fn new(inbound_messaging: TConsensusSpec::InboundMessaging) -> Self {
        Self {
            buffer: BTreeMap::new(),
            inbound_messaging,
        }
    }

    pub async fn next(
        &mut self,
        current_epoch: Epoch,
        current_height: NodeHeight,
        has_processed_first_block: bool,
    ) -> IncomingMessageResult<TConsensusSpec::Addr> {
        let next_height = current_height + NodeHeight(1);
        // Clear buffer with lower (epoch, heights)
        let before_len = self.buffer.len();
        self.buffer = self.buffer.split_off(&(current_epoch, next_height));
        let after_len = self.buffer.len();

        debug!(
            target: LOG_TARGET,
            "Next message for current view {}/{} (has_processed_first_block={}, discard={})",
            current_epoch,
            current_height,
            has_processed_first_block,
            before_len - after_len
        );
        if has_processed_first_block {
            // Drain all buffered messages for the current view
            if let Some(buffer) = self.buffer.get_mut(&(current_epoch, next_height)) &&
                let Some(msg_tuple) = buffer.pop_front()
            {
                return Ok(Some(msg_tuple));
            }
        }

        while let Some(result) = self.inbound_messaging.next_message().await {
            let (from, msg) = result?;

            // If the message is for two epochs or more ahead or behind, discard it.
            if msg.epoch() > current_epoch + Epoch(1) ||
                current_epoch.checked_sub(Epoch(1)).is_some_and(|e| msg.epoch() < e)
            {
                warn!(
                    target: LOG_TARGET,
                    "🗑️ Discard non-applicable message {} for epoch {}. Current epoch is {}",
                    msg,
                    msg.epoch(),
                    current_epoch
                );
                continue;
            }

            match msg_relative_view(&msg, current_epoch, current_height, has_processed_first_block) {
                MessageRelativeView::Current => {
                    return Ok(Some((from, msg)));
                },
                MessageRelativeView::Past { epoch, height } => {
                    info!(target: LOG_TARGET, "🗑️ Discard message {} is for previous view {}/{}. Current view {}/{}", msg, epoch, height, current_epoch, current_height);
                },
                MessageRelativeView::Future { epoch, height } => {
                    // Buffer message for future epoch/height
                    if msg.proposal().is_some() {
                        info!(target: LOG_TARGET, "🔮 Proposal {msg} is for future view {height} (Current view: {current_epoch}, {current_height})");
                    } else {
                        info!(target: LOG_TARGET, "🔮 Message {msg} is for future view {height} (Current view: {current_epoch}, {current_height})");
                    }
                    self.push_to_buffer(epoch, height, from, msg);
                },
                MessageRelativeView::Discard => {
                    warn!(target: LOG_TARGET, "🗑️ Discard non-applicable message {}. Current view {}/{}", msg, current_epoch, current_height);
                },
            }
        }

        info!(
            target: LOG_TARGET,
            "Inbound messaging has terminated. Current view: {}/{}", current_epoch, current_height
        );
        // Inbound messaging has terminated
        Ok(None)
    }

    pub async fn discard(&mut self) {
        self.clear_buffer();
        while self.inbound_messaging.next_message().await.is_some() {}
    }

    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    fn push_to_buffer(&mut self, epoch: Epoch, height: NodeHeight, from: TConsensusSpec::Addr, msg: HotstuffMessage) {
        const MAX_BUFFERED_MESSAGES_PER_VIEW: usize = 1000;
        const MAX_BUFFERED_VIEWS: usize = 100_000;
        if self.buffer.len() >= MAX_BUFFERED_VIEWS {
            warn!(
                target: LOG_TARGET,
                "🗑️ Discarding message {} for view {}/{} as buffer view limit of {} reached",
                msg,
                epoch,
                height,
                MAX_BUFFERED_VIEWS
            );
            return;
        }

        let messages_mut = self.buffer.entry((epoch, height)).or_default();
        if messages_mut.len() > MAX_BUFFERED_MESSAGES_PER_VIEW {
            warn!(
                target: LOG_TARGET,
                "🗑️ Discarding message {} for view {}/{} as buffer limit of {} reached",
                msg,
                epoch,
                height,
                MAX_BUFFERED_MESSAGES_PER_VIEW
            );
            return;
        }
        messages_mut.push_back((from, msg));
    }
}

enum MessageRelativeView {
    /// The message is for the current view, or is applicable to the current view
    Current,
    /// The message is for a past view
    Past { epoch: Epoch, height: NodeHeight },
    /// The message is for a future view
    Future { epoch: Epoch, height: NodeHeight },
    /// The message is not and will never be applicable
    Discard,
}

#[allow(clippy::too_many_lines)]
fn msg_relative_view(
    msg: &HotstuffMessage,
    current_epoch: Epoch,
    current_height: NodeHeight,
    has_processed_first_block: bool,
) -> MessageRelativeView {
    match msg {
        HotstuffMessage::Proposal(msg) => {
            let next_height = current_height + NodeHeight(1);
            let epoch = msg.block.epoch();
            let block_height = msg.block.height();
            let pc_height = msg.block.justify().height();

            if epoch < current_epoch {
                return MessageRelativeView::Past {
                    epoch: msg.block.epoch(),
                    height: pc_height,
                };
            }

            if epoch == current_epoch + Epoch(1) {
                return MessageRelativeView::Future {
                    epoch,
                    height: pc_height,
                };
            }

            if epoch > current_epoch {
                return MessageRelativeView::Discard;
            }

            if pc_height <= current_height &&
                msg.block
                    .timeout_certificate()
                    .is_some_and(|tc| tc.height() > current_height)
            {
                return MessageRelativeView::Current;
            }

            if pc_height < current_height || (pc_height == current_height && block_height <= current_height) {
                return MessageRelativeView::Past {
                    epoch,
                    height: pc_height,
                };
            }

            if has_processed_first_block {
                if pc_height > next_height {
                    let msg_height = msg
                        .block
                        .timeout_certificate()
                        .map(|_| pc_height + NodeHeight(1))
                        .unwrap_or(pc_height);

                    return MessageRelativeView::Future {
                        epoch,
                        height: msg_height,
                    };
                }
            } else {
                // (a) Special case, the justify height and the current height are zero, and we have not processed the
                // first block This is specifically to handle the case where we are starting from
                // genesis and immediately run a catch up. The first and second blocks both are for view
                // 1. If has_processed_first_block is false for the second block, we want to process it
                // in the future not immediately (which would happen in (c) below).
                // TODO: hacky
                if pc_height == NodeHeight(1) && block_height == NodeHeight(2) {
                    return MessageRelativeView::Future {
                        epoch,
                        height: NodeHeight(1),
                    };
                }
                if block_height > NodeHeight(1) {
                    return MessageRelativeView::Future {
                        epoch,
                        height: pc_height,
                    };
                }
            }

            MessageRelativeView::Current
        },
        HotstuffMessage::Vote(msg) => {
            let vote = &msg.vote;
            let vote_view_height = vote.height();
            if vote.epoch() == current_epoch && vote_view_height >= current_height {
                return MessageRelativeView::Current;
            }

            if vote.epoch() < current_epoch || (vote.epoch() == current_epoch && vote_view_height < current_height) {
                return MessageRelativeView::Past {
                    epoch: vote.epoch(),
                    height: vote_view_height,
                };
            }

            // Epoch >= current_epoch
            MessageRelativeView::Future {
                epoch: vote.epoch(),
                height: vote_view_height,
            }
        },
        HotstuffMessage::NewView(msg) => {
            // let Some(height) = msg.max_height().checked_add(NodeHeight(1)) else {
            //     warn!(
            //         target: LOG_TARGET,
            //         "❗️ NewView message {} has invalid max height {}. Discarding.", msg, msg.max_height()
            //     );
            //     return MessageRelativeView::Discard;
            // };

            let height = msg.max_height();
            if msg.epoch() < current_epoch || (msg.epoch() == current_epoch && height < current_height) {
                MessageRelativeView::Past {
                    epoch: msg.epoch(),
                    height,
                }
            } else {
                // All new view messages for future view heights are considered applicable and should be processed
                MessageRelativeView::Current
            }
        },
        HotstuffMessage::ForeignProposal(_) => {
            // Foreign proposals are always applicable
            MessageRelativeView::Current
        },
        HotstuffMessage::ForeignProposalNotification(_) => MessageRelativeView::Current,
        HotstuffMessage::ForeignProposalRequest(_) => MessageRelativeView::Current,
        HotstuffMessage::MissingTransactionsRequest(msg) => {
            if msg.epoch < current_epoch {
                return MessageRelativeView::Past {
                    epoch: msg.epoch,
                    height: NodeHeight::zero(),
                };
            }
            if msg.epoch > current_epoch {
                return MessageRelativeView::Future {
                    epoch: msg.epoch,
                    height: NodeHeight::zero(),
                };
            }
            MessageRelativeView::Current
        },
        HotstuffMessage::MissingTransactionsResponse(msg) => {
            if msg.epoch < current_epoch {
                return MessageRelativeView::Past {
                    epoch: msg.epoch,
                    height: NodeHeight::zero(),
                };
            }
            if msg.epoch > current_epoch {
                return MessageRelativeView::Future {
                    epoch: msg.epoch,
                    height: NodeHeight::zero(),
                };
            }
            MessageRelativeView::Current
        },
        HotstuffMessage::CatchUpSyncRequest(msg) => {
            if msg.epoch != current_epoch {
                return MessageRelativeView::Discard;
            }
            MessageRelativeView::Current
        },
    }
}
