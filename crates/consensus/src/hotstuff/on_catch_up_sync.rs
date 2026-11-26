//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::{info, warn};
use tari_consensus_types::HighPc;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{StateStore, consensus_models::BookkeepingModel};

use crate::{
    hotstuff::{HotStuffError, View},
    messages::{CatchUpRequestMessage, HotstuffMessage},
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_catch_up_sync";

pub struct OnCatchUpSync<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
}

impl<TConsensusSpec: ConsensusSpec> OnCatchUpSync<TConsensusSpec> {
    pub fn new(store: TConsensusSpec::StateStore, outbound_messaging: TConsensusSpec::OutboundMessaging) -> Self {
        Self {
            store,
            outbound_messaging,
        }
    }

    /// Asks `to` for the page of blocks of `epoch` starting at `from_height`, past the `skip` blocks at that height
    /// already received. Without a height, the page starts at this node's highest certified block.
    pub async fn request_page(
        &mut self,
        current_view: View,
        epoch: Epoch,
        to: TConsensusSpec::Addr,
        request_id: u32,
        from_height: Option<NodeHeight>,
        skip: u32,
    ) -> Result<(), HotStuffError> {
        let (from_height, skip) = match from_height {
            Some(height) => (height, skip),
            None => {
                let high_qc = self.store.with_read_tx(|tx| HighPc::get(tx, epoch))?;
                let height = if high_qc.epoch() == epoch {
                    high_qc.height()
                } else {
                    NodeHeight(1)
                };
                (height, 0)
            },
        };

        info!(
            target: LOG_TARGET,
            "⏰ Catch up page {request_id} from block {epoch}/{from_height} (skip {skip}) from {to} (current view: \
             {current_view})",
        );

        if self
            .outbound_messaging
            .send(
                to,
                HotstuffMessage::CatchUpSyncRequest(CatchUpRequestMessage {
                    request_id,
                    epoch,
                    from_height,
                    skip,
                }),
            )
            .await
            .is_err()
        {
            warn!(target: LOG_TARGET, "Leader channel closed while sending SyncRequest");
        }
        Ok(())
    }
}
