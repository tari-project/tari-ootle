//  Copyright 2021. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use log::*;
use tari_consensus::hotstuff::HotstuffEvent;
use tari_dan_app_utilities::template_manager::interface::TemplateExecutable;
use tari_dan_storage::{
    consensus_models::{Block, Decision},
    StateStore,
};
use tari_engine_types::{
    commit_result::TransactionResult,
    substate::{SubstateId, SubstateValue},
};
use tari_epoch_manager::{EpochManagerEvent, EpochManagerReader};
use tari_networking::NetworkingService;
use tari_shutdown::ShutdownSignal;

use crate::Services;

const LOG_TARGET: &str = "tari::validator_node::dan_node";

pub struct DanNode {
    services: Services,
}

impl DanNode {
    pub fn new(services: Services) -> Self {
        Self { services }
    }

    pub async fn start(mut self, mut shutdown: ShutdownSignal) -> Result<(), anyhow::Error> {
        let mut hotstuff_events = self.services.consensus_handle.subscribe_to_hotstuff_events();
        let mut epoch_manager_events = self.services.epoch_manager.subscribe();

        // if let Err(err) = self.dial_local_shard_peers().await {
        //     error!(target: LOG_TARGET, "Failed to dial local shard peers: {}", err);
        // }

        loop {
            tokio::select! {
                // Wait until killed
                _ = shutdown.wait() => {
                     break;
                },

                Ok(event) = hotstuff_events.recv() => if let Err(err) = self.handle_hotstuff_event(event).await{
                    error!(target: LOG_TARGET, "Error handling hotstuff event: {}", err);
                },

                Ok(event) = epoch_manager_events.recv() => if let Err(err) = self.handle_epoch_manager_event(event).await{
                    error!(target: LOG_TARGET, "Error handling epoch manager event: {}", err);
                },

                Err(err) = self.services.on_any_exit() => {
                    error!(target: LOG_TARGET, "Error in service: {}", err);
                    return Err(err);
                }

            }
        }

        Ok(())
    }

    async fn handle_epoch_manager_event(&mut self, event: EpochManagerEvent) -> Result<(), anyhow::Error> {
        let EpochManagerEvent::EpochChanged { epoch, .. } = event;
        let all_vns = self.services.epoch_manager.get_all_validator_nodes(epoch).await?;
        self.services
            .networking
            .set_want_peers(all_vns.into_iter().map(|vn| vn.address.as_peer_id()))
            .await?;

        Ok(())
    }

    /// Handles template publishes, adds all the committed templates to template manager
    /// from the given block.
    async fn handle_template_publishes(&self, block: &Block) -> Result<(), anyhow::Error> {
        // add wasm templates to template manager if available in any of the new block's transactions
        let templates = self
            .services
            .state_store
            .with_read_tx(|tx| block.get_transactions(tx))?
            .into_iter()
            .filter(|record| matches!(record.final_decision, Some(Decision::Commit)))
            .filter_map(|record| {
                let result = record.execution_result?;
                let TransactionResult::Accept(diff) = result.finalize.result else {
                    return None;
                };
                let tx_signature = record.transaction.signatures().first()?;
                let signer_pub_key = tx_signature.public_key().clone();
                Some((signer_pub_key, diff))
            })
            .flat_map(|(signer_pub_key, diff)| {
                let mut templates = vec![];
                for (substate_id, substate) in diff.into_up_iter() {
                    if let SubstateId::Template(template_address) = substate_id {
                        let template_address_hash = template_address.as_hash();
                        if let SubstateValue::Template(template) = substate.into_substate_value() {
                            templates.push((
                                signer_pub_key.clone(),
                                template_address_hash,
                                TemplateExecutable::CompiledWasm(template.binary),
                            ));
                        }
                    }
                }
                templates
            });

        // adding templates to template manager
        let mut template_counter = 0;
        for (author_pub_key, template_address, template) in templates {
            self.services
                .template_manager
                .add_template(author_pub_key, template_address, template, None)
                .await?;
            template_counter += 1;
        }

        if template_counter > 0 {
            info!(target: LOG_TARGET, "🏁 {} new template(s) have been persisted locally.", template_counter);
        }

        Ok(())
    }

    async fn handle_hotstuff_event(&self, event: HotstuffEvent) -> Result<(), anyhow::Error> {
        info!(target: LOG_TARGET, "🔥 consensus event: {event}");

        let HotstuffEvent::BlockCommitted { block_id, .. } = event else {
            return Ok(());
        };

        let block = self.services.state_store.with_read_tx(|tx| Block::get(tx, &block_id))?;

        info!(target: LOG_TARGET, "🏁 Block {} committed", block);

        self.handle_template_publishes(&block).await?;

        let committed_transactions = block
            .commands()
            .iter()
            .filter_map(|cmd| cmd.committing())
            .map(|t| t.id)
            .collect::<Vec<_>>();

        if committed_transactions.is_empty() {
            return Ok(());
        }

        info!(target: LOG_TARGET, "🏁 Removing {} finalized transaction(s) from mempool", committed_transactions.len());
        if let Err(err) = self.services.mempool.remove_transactions(committed_transactions).await {
            error!(target: LOG_TARGET, "Failed to remove transaction from mempool: {}", err);
        }

        Ok(())
    }

    // async fn dial_local_shard_peers(&mut self) -> Result<(), anyhow::Error> {
    //     let epoch = self.services.epoch_manager.current_epoch().await?;
    //     let res = self
    //         .services
    //         .epoch_manager
    //         .get_validator_node(epoch, &self.services.networking.local_peer_id().into())
    //         .await;
    //
    //     let shard_id = match res {
    //         Ok(vn) => vn.shard_key,
    //         Err(EpochManagerError::ValidatorNodeNotRegistered { address, epoch }) => {
    //             info!(target: LOG_TARGET, "Validator node {address} not registered for current epoch {epoch}");
    //             return Ok(());
    //         },
    //         Err(EpochManagerError::BaseLayerConsensusConstantsNotSet) => {
    //             info!(target: LOG_TARGET, "Epoch manager has not synced with base layer yet");
    //             return Ok(());
    //         },
    //         Err(err) => {
    //             return Err(err.into());
    //         },
    //     };
    //
    //     let local_shard_peers = self.services.epoch_manager.get_committee(epoch, shard_id).await?;
    //     info!(
    //         target: LOG_TARGET,
    //         "Dialing {} local shard peers",
    //         local_shard_peers.members.len()
    //     );
    //     let local_peer_id = *self.services.networking.local_peer_id();
    //     let local_shard_peers = local_shard_peers.addresses().filter(|addr| **addr != local_peer_id);
    //
    //     for peer in local_shard_peers {
    //         if let Err(err) = self.services.networking.dial_peer(peer.to_peer_id()).await {
    //             debug!(target: LOG_TARGET, "Failed to dial peer: {}", err);
    //         }
    //     }
    //
    //     Ok(())
    // }
}
