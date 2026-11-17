//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::Serialize;
use tari_ootle_common_types::{Epoch, NodeHeight};

#[derive(Debug, Clone, Serialize)]
pub struct CatchUpRequestMessage {
    pub epoch: Epoch,
    pub block_height: NodeHeight,
}

/// Ends the response to a catch-up request. It follows the last block sent, or stands alone when the responder sends
/// none, so the requester knows the stream is over.
#[derive(Debug, Clone, Serialize)]
pub struct CatchUpDoneMessage {
    pub epoch: Epoch,
    /// The height of the last block sent, which is the responder's leaf when the whole range went out. `None` when
    /// no block was sent.
    pub end_height: Option<NodeHeight>,
}
