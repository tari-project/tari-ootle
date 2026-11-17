//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Serialize;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::messages::{EncodedTransaction, ProposalMessage};

/// The most blocks a catch-up page carries. The transport frame bounds a page's bytes, and this bounds the work of
/// decoding one.
pub const MAX_CATCH_UP_PAGE_BLOCKS: usize = 1000;

/// Asks a committee member for one page of the blocks of `epoch` from `from_height`. Every request is answered by
/// exactly one [`CatchUpPageMessage`] carrying the same `request_id`.
#[derive(Debug, Clone, Serialize)]
pub struct CatchUpRequestMessage {
    pub request_id: u32,
    pub epoch: Epoch,
    pub from_height: NodeHeight,
    /// The number of blocks at `from_height` already received, in the order the responder sends a height's forks.
    pub skip: u32,
}

/// One page of blocks in answer to a [`CatchUpRequestMessage`], filled up to the responder's byte budget.
#[derive(Debug, Clone, Serialize)]
pub struct CatchUpPageMessage {
    pub request_id: u32,
    pub epoch: Epoch,
    /// Certified blocks from the requested height, in height order.
    pub blocks: Vec<ProposalMessage>,
    /// The responder's leaf block, when the page reaches it. It has no certificate yet, so the requester handles it as
    /// a live proposal and may vote on it.
    pub leaf: Option<ProposalMessage>,
    /// Transactions the page's blocks commit to, as many as the budget leaves room for. The requester fetches any
    /// others the usual way.
    pub transactions: Vec<EncodedTransaction>,
    /// The height the next page starts at, or `None` when this page reaches the responder's leaf.
    pub next_height: Option<NodeHeight>,
    /// The number of blocks at `next_height` the requester holds after this page: those its request skipped and those
    /// this page carries. The next request to this responder skips them.
    pub next_skip: u32,
}

impl CatchUpPageMessage {
    /// A page carrying no blocks, for a request the responder cannot serve.
    pub fn empty(request_id: u32, epoch: Epoch) -> Self {
        Self {
            request_id,
            epoch,
            blocks: Vec::new(),
            leaf: None,
            transactions: Vec::new(),
            next_height: None,
            next_skip: 0,
        }
    }

    /// The highest block the page carries.
    pub fn end_height(&self) -> Option<NodeHeight> {
        self.blocks
            .iter()
            .chain(self.leaf.iter())
            .map(|proposal| proposal.block.height())
            .max()
    }
}
