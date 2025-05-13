//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::{info, warn};
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{consensus_models::HighQc, StateStore};

use crate::{
    hotstuff::{pacemaker_handle::PaceMakerHandle, HotStuffError},
    messages::{HotstuffMessage, SyncRequestMessage},
    traits::{ConsensusSpec, OutboundMessaging},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_catch_up_sync";

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
        let high_qc = self.store.with_read_tx(|tx| HighQc::get(tx, epoch))?;

        let block_height = if high_qc.epoch() == epoch {
            high_qc.block_height()
        } else {
            NodeHeight::zero()
        };

        // Reset leader timeout to previous height since we're behind and need to process catch up blocks. This is the
        // only case where the view is non-monotonic. TODO: is this correct/necessary?
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
                HotstuffMessage::CatchUpSyncRequest(SyncRequestMessage { epoch, block_height }),
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
