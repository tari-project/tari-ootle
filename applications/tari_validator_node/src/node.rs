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

use std::{process, time::Duration};

use anyhow::Context;
use log::*;
use tari_consensus::hotstuff::HotstuffEvent;
use tari_epoch_manager::{EpochManagerEvent, EpochManagerReader};
use tari_networking::NetworkingService;
use tari_ootle_storage::{StateStore, consensus_models::Block};
use tari_shutdown::Shutdown;

// use tokio::signal::unix::{signal, SignalKind};
use crate::{Services, ValidatorNodeStateStore};

const LOG_TARGET: &str = "tari::validator_node";
lazy_static::lazy_static! {
    static ref PANIC_NOTIFIER: tokio::sync::Notify = tokio::sync::Notify::new();
}

pub fn trigger_panic_notifier() {
    PANIC_NOTIFIER.notify_waiters();
}

pub struct ValidatorNode {
    services: Services<ValidatorNodeStateStore>,
}

impl ValidatorNode {
    pub fn new(services: Services<ValidatorNodeStateStore>) -> Self {
        Self { services }
    }

    pub async fn start(mut self, mut shutdown: Shutdown) -> Result<(), anyhow::Error> {
        let mut hotstuff_events = self.services.consensus_handle.subscribe_to_hotstuff_events()?;
        let mut epoch_manager_events = self.services.epoch_manager.subscribe();

        loop {
            let metrics = tokio::runtime::Handle::current().metrics();
            info!(
                target: LOG_TARGET,
                "Tokio runtime metrics: num_alive_tasks={}, num_workers={}, global_queue_depth={}",
                metrics.num_alive_tasks(),
                metrics.num_workers(),
                metrics.global_queue_depth(),
            );

            tokio::select! {
                _ = PANIC_NOTIFIER.notified() => {
                    error!(target: LOG_TARGET, "💤 Panic detected in another task. Shutting down...");
                    shutdown.trigger();
                    break;
                },
                _ = tokio::signal::ctrl_c() => {
                    info!(target: LOG_TARGET, "💤 Received SIGINT");
                    shutdown.trigger();
                    break;
                },

                Ok(event) = hotstuff_events.recv() => if let Err(err) = self.handle_hotstuff_event(event).await {
                    error!(target: LOG_TARGET, "Error handling hotstuff event: {}", err);
                },

                Ok(event) = epoch_manager_events.recv() => if let Err(err) = self.handle_epoch_manager_event(event).await {
                    error!(target: LOG_TARGET, "Error handling epoch manager event: {}", err);
                },

                result = self.services.on_any_exit() => {
                    match result {
                        Ok(_) => {
                            if !shutdown.is_triggered() {
                                warn!(target: LOG_TARGET, "❓️ A service has exited unexpectedly. Shutting down...");
                            }
                            shutdown.trigger();
                            break;
                        },
                        Err(err) => {
                            error!(target: LOG_TARGET, "Error in service: {}", err);
                            return Err(err);
                        }
                    }
                }
            }
        }

        // Just exit ASAP on panic
        info!(target: LOG_TARGET, "💤 Waiting for all services to shut down... ctrl+c to force shutdown");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                // Second SIGINT forces shutdown
                warn!(target: LOG_TARGET, "💤 Shutdown NOW");
                process::exit(1);
            },
            res = tokio::time::timeout(Duration::from_secs(20), self.services.join_all()) => {
                res.context("Timeout waiting for all workers to end")??;
                info!(target: LOG_TARGET, "🏁 All services have exited cleanly");
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

    async fn handle_hotstuff_event(&self, event: HotstuffEvent) -> Result<(), anyhow::Error> {
        info!(target: LOG_TARGET, "🔥 consensus event: {event}");

        let HotstuffEvent::BlockCommitted { block_id, .. } = event else {
            return Ok(());
        };

        let block = self.services.state_store.with_read_tx(|tx| Block::get(tx, &block_id))?;

        info!(target: LOG_TARGET, "🏁 Block {} committed", block);

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
}
