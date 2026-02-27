//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::{info, warn};
use tari_consensus_types::HighPc;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{StateStore, consensus_models::BookkeepingModel};

use crate::{
    hotstuff::{HotStuffError, pacemaker_handle::PaceMakerHandle},
    messages::{CatchUpRequestMessage, HotstuffMessage},
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_catch_up_sync";

pub struct OnCatchUpSync<TConsensusSpec: ConsensusSpec> {
    store: TConsensusSpec::StateStore,
    pacemaker: PaceMakerHandle,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
}

impl<TConsensusSpec: ConsensusSpec> OnCatchUpSync<TConsensusSpec> {
    pub fn new(
        store: TConsensusSpec::StateStore,
        pacemaker: PaceMakerHandle,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
    ) -> Self {
        Self {
            store,
            pacemaker,
            outbound_messaging,
        }
    }

    pub async fn request_sync(&mut self, epoch: Epoch, from: TConsensusSpec::Addr) -> Result<(), HotStuffError> {
        let high_qc = self.store.with_read_tx(|tx| HighPc::get(tx, epoch))?;

        let block_height = if high_qc.epoch() == epoch {
            high_qc.height()
        } else {
            NodeHeight(1)
        };

        // Reset leader timeout to previous height since we're behind and need to process catch up blocks. This is the
        // only case where the view is non-monotonic.
        self.pacemaker.reset_view(epoch, block_height, block_height).await?;

        info!(
            target: LOG_TARGET,
            "⏰ Catch up required from block {}/{} from {} (current view: {})",
            epoch,
            block_height,
            from,
            self.pacemaker.current_view()
        );

        // Request a catch-up
        if self
            .outbound_messaging
            .send(
                from,
                HotstuffMessage::CatchUpSyncRequest(CatchUpRequestMessage { epoch, block_height }),
            )
            .await
            .is_err()
        {
            warn!(target: LOG_TARGET, "Leader channel closed while sending SyncRequest");
            return Ok(());
        }

        Ok(())
    }
}
