//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{BTreeMap, VecDeque};

use log::*;
use tari_consensus_types::Vote;
use tari_ootle_common_types::{Epoch, NodeAddressable, NodeHeight};

use crate::{
    hotstuff::error::HotStuffError,
    messages::HotstuffMessage,
    traits::{hooks::ConsensusHooks, ConsensusSpec, InboundMessaging},
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
    ) -> Option<Result<(TConsensusSpec::Addr, HotstuffMessage), HotStuffError>> {
        // Then incoming messages for the current epoch/height
        let result = self.message_buffer.next(current_epoch, current_height).await;
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
    ) -> IncomingMessageResult<TConsensusSpec::Addr> {
        // We listen for messages for the next view
        let next_height = current_height; // + NodeHeight(1);
                                          // Clear buffer with lower (epoch, heights)
        self.buffer = self.buffer.split_off(&(current_epoch, next_height));

        // Drain all buffered messages for the current view
        if let Some(buffer) = self.buffer.get_mut(&(current_epoch, next_height)) {
            if let Some(msg_tuple) = buffer.pop_front() {
                return Ok(Some(msg_tuple));
            }
        }

        while let Some(result) = self.inbound_messaging.next_message().await {
            let (from, msg) = result?;

            // If we receive an FP that is greater than our current epoch, we buffer it. The height is not relevant.
            if let HotstuffMessage::ForeignProposal(ref m) = msg {
                if m.proposal.epoch() > current_epoch {
                    self.push_to_buffer(m.proposal.epoch(), NodeHeight::zero(), from, msg);
                    continue;
                }
            }

            match msg_relative_view(&msg, current_epoch, next_height) {
                MessageRelativeView::Current => {
                    return Ok(Some((from, msg)));
                },
                MessageRelativeView::Past { epoch, height } => {
                    info!(target: LOG_TARGET, "🗑️ Discard message {} is for previous view {}/{}. Current view {}/{}", msg, epoch, height, current_epoch, current_height);
                },
                MessageRelativeView::Future { epoch, height } => {
                    // Buffer message for future epoch/height
                    if msg.proposal().is_some() {
                        info!(target: LOG_TARGET, "🔮 Proposal {msg} is for future view (Current view: {current_epoch}, {current_height})");
                    } else {
                        info!(target: LOG_TARGET, "🔮 Message {msg} is for future view (Current view: {current_epoch}, {current_height})");
                    }
                    self.push_to_buffer(epoch, height, from, msg);
                },
                MessageRelativeView::Discard => {
                    info!(target: LOG_TARGET, "🗑️ Discard non-applicable message {}. Current view {}/{}", msg, current_epoch, current_height);
                },
            }
        }

        info!(
            target: LOG_TARGET,
            "Inbound messaging has terminated. Current view: {}/{}", current_epoch, next_height
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
        self.buffer.entry((epoch, height)).or_default().push_back((from, msg));
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Needs sync: local height {local_height} is less than remote QC height {qc_height} from {from}")]
pub struct NeedsSync<TAddr: NodeAddressable> {
    pub from: TAddr,
    pub local_height: NodeHeight,
    pub qc_height: NodeHeight,
    pub remote_epoch: Epoch,
    pub local_epoch: Epoch,
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
fn msg_relative_view(msg: &HotstuffMessage, current_epoch: Epoch, current_height: NodeHeight) -> MessageRelativeView {
    match msg {
        HotstuffMessage::Proposal(msg) => {
            let next_height = current_height + NodeHeight(1);
            let epoch = msg.block.epoch();
            let block_height = msg.block.height();
            let justify_height = msg.block.max_certificate_height();
            if epoch == current_epoch &&
                // Special case, the justify height and the current height are zero
                ((current_height.is_zero() && justify_height.is_zero()) ||
                    // The proposal (supposedly) justifies the next view change
                    justify_height >= next_height ||
                    // The proposal is for the current view (catch up)
                    block_height == next_height ||
                    // or, the timeout certificate height is higher than the current height
                    msg.block
                        .timeout_certificate()
                        .is_some_and(|t| t.height() >= current_height))
            {
                return MessageRelativeView::Current;
            }

            if epoch < current_epoch || (epoch == current_epoch && justify_height < next_height) {
                return MessageRelativeView::Past {
                    epoch: msg.block.epoch(),
                    height: justify_height,
                };
            }

            MessageRelativeView::Future {
                epoch: msg.block.epoch(),
                height: justify_height,
            }
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
        HotstuffMessage::ForeignProposal(msg) => {
            if msg.proposal.epoch() < current_epoch {
                return MessageRelativeView::Past {
                    epoch: msg.proposal.epoch(),
                    height: msg.proposal.height(),
                };
            }
            if msg.proposal.epoch() > current_epoch {
                return MessageRelativeView::Future {
                    epoch: msg.proposal.epoch(),
                    height: msg.proposal.height(),
                };
            }
            MessageRelativeView::Current
        },
        HotstuffMessage::ForeignProposalNotification(msg) => {
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
        HotstuffMessage::ForeignProposalRequest(msg) => {
            if msg.epoch() < current_epoch {
                return MessageRelativeView::Past {
                    epoch: msg.epoch(),
                    height: NodeHeight::zero(),
                };
            }
            if msg.epoch() > current_epoch {
                return MessageRelativeView::Future {
                    epoch: msg.epoch(),
                    height: NodeHeight::zero(),
                };
            }
            MessageRelativeView::Current
        },
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
