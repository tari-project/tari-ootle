//  Copyright 2022. The Tari Project
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

use std::{
    mem,
    sync::{atomic::AtomicU64, Arc},
};

use log::*;
use tari_common_types::types::FixedHash;
use tari_ootle_common_types::{
    displayable::Displayable,
    optional::{IsNotFoundError, Optional},
    Epoch,
    VotePower,
};
use tari_ootle_storage::global::GlobalDb;
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_shutdown::ShutdownSignal;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    epoch_event_oracle::{EpochEvent, EpochEventOracle, ValidatorNodeChange},
    error::EpochManagerError,
    service::{
        config::EpochManagerConfig,
        epoch_manager::EpochManager,
        types::EpochManagerRequest,
        EpochManagerHandle,
    },
    traits::{EpochManagerSpec, TemplateDownloader},
    EpochManagerEvent,
};

const LOG_TARGET: &str = "tari::ootle::epoch_manager";

pub struct EpochManagerService<TSpec: EpochManagerSpec> {
    rx_request: mpsc::Receiver<EpochManagerRequest<TSpec::Addr>>,
    inner: EpochManager<TSpec>,
    epoch_events: TSpec::EpochEventOracle,
    template_downloader: TSpec::TemplateDownloader,

    tx_events: broadcast::Sender<EpochManagerEvent>,
    is_initial_epoch_sync_complete: bool,
    has_epoch_changed: bool,
    waiting_for_scanning_complete: Vec<oneshot::Sender<Result<(), EpochManagerError>>>,

    shutdown: ShutdownSignal,
}

impl<TSpec: EpochManagerSpec> EpochManagerService<TSpec> {
    pub fn spawn(
        config: EpochManagerConfig,
        global_db: GlobalDb<SqliteGlobalDbAdapter<TSpec::Addr>>,
        epoch_events: TSpec::EpochEventOracle,
        template_downloader: TSpec::TemplateDownloader,
        layer_one_transaction_submitter: TSpec::LayerOneSubmitter,
        node_public_key: RistrettoPublicKeyBytes,
        shutdown: ShutdownSignal,
    ) -> (EpochManagerHandle<TSpec::Addr>, JoinHandle<anyhow::Result<()>>) {
        let (tx_request, rx_request) = mpsc::channel(10);
        let (events, _) = broadcast::channel(100);
        let current_epoch = Arc::new(AtomicU64::new(0));
        let epoch_manager_handle = EpochManagerHandle::new(tx_request, events.downgrade(), current_epoch.clone());

        let task_handle = tokio::spawn(async move {
            Self {
                rx_request,
                inner: EpochManager::new(
                    config,
                    global_db,
                    layer_one_transaction_submitter,
                    node_public_key,
                    current_epoch,
                ),
                tx_events: events,
                has_epoch_changed: false,
                is_initial_epoch_sync_complete: false,
                waiting_for_scanning_complete: Vec::new(),
                epoch_events,
                template_downloader,
                shutdown,
            }
            .run()
            .await?;
            Ok(())
        });

        (epoch_manager_handle, task_handle)
    }

    pub async fn run(&mut self) -> Result<(), EpochManagerError> {
        info!(target: LOG_TARGET, "Starting epoch manager");
        // first, load initial state
        self.inner.load_initial_state()?;

        loop {
            tokio::select! {
                maybe_event = self.epoch_events.next_epoch_event() => {
                    match maybe_event {
                        Some(event) => {
                            if let Err(err) = self.handle_epoch_event(event).await {
                                error!(target: LOG_TARGET, "🚨 Epoch event error: {err}");
                            }
                        }
                        None => {
                            warn!(target: LOG_TARGET, "💤 Shutting down epoch manager (no further epoch events)");
                            break;
                        }
                    }
                },

                req = self.rx_request.recv() => {
                    match req {
                        Some(req) => self.handle_request(req).await,
                        None => {
                            error!(target: LOG_TARGET, "Channel closed. Shutting down epoch manager");
                            break;
                        }
                    }
                },

                _ = self.shutdown.wait() => {
                    info!(target: LOG_TARGET, "💤 Shutting down epoch manager (shutdown signal)");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_epoch_event(&mut self, event: EpochEvent) -> anyhow::Result<()> {
        match event {
            EpochEvent::Error(err) => return Err(err),
            EpochEvent::ActiveValidatorNodeSetChanged { epoch, node_changes } => {
                info!(
                    target: LOG_TARGET,
                    "⛓️ {} validator node change(s) for epoch {}", node_changes.len(), epoch,
                );

                for node_change in node_changes {
                    match node_change {
                        ValidatorNodeChange::Add {
                            claim_public_key,
                            validator_node_public_key,
                            activation_epoch,
                            minimum_value_promise: _minimum_value_promise,
                            shard_key,
                        } => {
                            info!(
                                target: LOG_TARGET,
                                "⛓️ Validator node {} activated at {}",
                                validator_node_public_key,
                                activation_epoch,
                            );

                            self.inner.add_validator_node_registration(
                                activation_epoch,
                                validator_node_public_key,
                                claim_public_key,
                                shard_key,
                                // minimum_value_promise,
                                // All validators currently get a vote power of 1
                                VotePower::of(1),
                            )?;
                        },
                        ValidatorNodeChange::Remove { public_key } => {
                            info!(
                                target: LOG_TARGET,
                                "⛓️ Deactivating validator node registration for {}",
                                public_key,
                            );

                            self.inner.deactivate_validator_node(public_key, epoch)?;
                        },
                    }
                }
            },
            EpochEvent::NewValidatorRegistered {
                epoch,
                validator_node_public_key,
                ..
            } => {
                // TODO: This occurs when a registration is detected, before the VN is activated and could be a good
                // time to start state sync
                info!(
                    target: LOG_TARGET,
                    "🖥️ New validator registered in {epoch} with public key {validator_node_public_key}",
                );
            },
            EpochEvent::NewValidatorNodeExit {
                epoch,
                validator_node_public_key,
                ..
            } => {
                info!(
                    target: LOG_TARGET,
                    "🖥️ validator exit in {epoch} with public key {validator_node_public_key}",
                );
            },
            EpochEvent::NewCodeTemplateDownload {
                epoch,
                name,
                address,
                author_public_key,
                url,
                binary_hash,
            } => {
                info!(
                    target: LOG_TARGET,
                    "🌠 new template found with address {address} at {epoch}",
                );
                self.template_downloader
                    .enqueue_download(epoch, name, address, author_public_key, url, binary_hash)
                    .await?
            },
            EpochEvent::NewBlockHeader { epoch, header } => {
                trace!(target: LOG_TARGET, "New block header at {epoch}: {header}");
                self.inner.insert_block_header(epoch, header)?;
            },
            EpochEvent::NewEvictionProof { epoch, eviction_proof } => {
                trace!(target: LOG_TARGET, "New Eviction proof for {epoch}: {eviction_proof:?}");
            },
            EpochEvent::EpochChanged { epoch, epoch_hash } => {
                info!(
                    target: LOG_TARGET,
                    "🌟 new epoch {epoch} with hash {epoch_hash}",
                );
                self.activate_epoch(epoch, epoch_hash)?;
            },

            EpochEvent::DoneForNow { epoch, .. } => {
                info!(target: LOG_TARGET, "Epoch event scanner done for now at {epoch}. Current epoch: {}", self.inner.current_epoch());
                self.on_scanning_complete()?;
            },
        }

        Ok(())
    }

    fn activate_epoch(&mut self, epoch: Epoch, epoch_hash: FixedHash) -> Result<(), EpochManagerError> {
        if self.current_epoch() >= epoch {
            // no need to update the epoch
            return Ok(());
        }

        self.has_epoch_changed = true;

        // In the base layer case, the epoch_hash is the first block of the epoch
        // persist the epoch data including the validator node set
        self.inner.insert_current_epoch(epoch, epoch_hash)?;
        self.inner.assign_validators_for_epoch(epoch)?;
        Ok(())
    }

    fn current_epoch(&self) -> Epoch {
        self.inner.current_epoch()
    }

    fn on_scanning_complete(&mut self) -> Result<(), EpochManagerError> {
        let current_epoch = self.inner.current_epoch();
        if !self.is_initial_epoch_sync_complete {
            info!(
                target: LOG_TARGET,
                "🌟 Initial epoch sync complete. Current epoch is {}", current_epoch
            );
            self.is_initial_epoch_sync_complete = true;
            for reply in mem::take(&mut self.waiting_for_scanning_complete) {
                let _ignore = reply.send(Ok(()));
            }
        }

        if self.has_epoch_changed {
            let num_committees = self.inner.get_number_of_committees(current_epoch)?;
            let shard_group = self.inner.get_our_validator_node(current_epoch).optional()?.map(|vn| {
                vn.shard_key
                    .to_shard_group(self.inner.config().num_preshards, num_committees)
            });
            let level = if self.is_initial_epoch_sync_complete {
                Level::Info
            } else {
                Level::Debug
            };
            log!(target: LOG_TARGET, level, "🌟 A new epoch {} is upon us. Shard group: {}", current_epoch, shard_group.display());

            self.publish_event(EpochManagerEvent::EpochChanged {
                epoch: current_epoch,
                registered_shard_group: shard_group,
            });
            self.has_epoch_changed = false;
        }

        Ok(())
    }

    fn publish_event(&mut self, event: EpochManagerEvent) {
        let _ignore = self.tx_events.send(event);
    }

    fn add_notify_on_scanning_complete(&mut self, reply: oneshot::Sender<Result<(), EpochManagerError>>) {
        if self.is_initial_epoch_sync_complete {
            let _ignore = reply.send(Ok(()));
        } else {
            self.waiting_for_scanning_complete.push(reply);
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_request(&mut self, req: EpochManagerRequest<TSpec::Addr>) {
        trace!(target: LOG_TARGET, "Received request: {:?}", req);
        let context = &format!("{:?}", req);
        match req {
            EpochManagerRequest::CurrentEpoch { reply } => handle(reply, Ok(self.inner.current_epoch()), context),
            EpochManagerRequest::CurrentEpochHash { reply } => {
                handle(reply, Ok(self.inner.current_epoch_hash()), context)
            },
            EpochManagerRequest::GetValidatorNodeByPublicKey {
                epoch,
                public_key,
                reply,
            } => handle(
                reply,
                self.inner
                    .get_validator_node_by_public_key(epoch, &public_key)
                    .and_then(|x| {
                        x.ok_or(EpochManagerError::ValidatorNodeNotRegistered {
                            address: public_key.to_string(),
                            epoch,
                        })
                    }),
                context,
            ),
            EpochManagerRequest::GetCommittees { epoch, reply } => {
                handle(reply, self.inner.get_committees(epoch), context);
            },
            EpochManagerRequest::GetCommitteeInfoByAddress { epoch, address, reply } => handle(
                reply,
                self.inner.get_committee_info_by_validator_address(epoch, address),
                context,
            ),
            EpochManagerRequest::GetCommitteeForSubstate {
                epoch,
                substate_address,
                reply,
            } => {
                handle(
                    reply,
                    self.inner.get_committee_for_substate(epoch, substate_address),
                    context,
                );
            },
            EpochManagerRequest::GetValidatorNodesPerEpoch { epoch, reply } => {
                handle(reply, self.inner.get_validator_nodes_per_epoch(epoch), context)
            },
            EpochManagerRequest::AddValidatorNodeRegistration {
                activation_epoch,
                validator_public_key,
                claim_public_key,
                shard_key,
                power,
                reply,
            } => handle(
                reply,
                self.inner.add_validator_node_registration(
                    activation_epoch,
                    validator_public_key,
                    claim_public_key,
                    shard_key,
                    power,
                ),
                context,
            ),
            EpochManagerRequest::DeactivateValidatorNode {
                public_key,
                deactivation_epoch,
                reply,
            } => handle(
                reply,
                self.inner.deactivate_validator_node(public_key, deactivation_epoch),
                context,
            ),
            EpochManagerRequest::IsInitialScanningComplete { reply } => {
                handle(reply, Ok(self.is_initial_epoch_sync_complete), context)
            },
            EpochManagerRequest::WaitForInitialScanningToComplete { reply } => {
                self.add_notify_on_scanning_complete(reply);
            },

            EpochManagerRequest::GetOurValidatorNode { epoch, reply } => {
                handle(reply, self.inner.get_our_validator_node(epoch), context)
            },
            EpochManagerRequest::GetCommitteeInfoForSubstate {
                epoch,
                substate_address,
                reply,
            } => handle(
                reply,
                self.inner.get_committee_info_for_substate(epoch, substate_address),
                context,
            ),
            EpochManagerRequest::GetLocalCommitteeInfo { epoch, reply } => {
                handle(reply, self.inner.get_local_committee_info(epoch), context)
            },
            EpochManagerRequest::GetCommitteeInfo {
                epoch,
                shard_group,
                reply,
            } => handle(reply, self.inner.get_committee_info(epoch, shard_group), context),
            EpochManagerRequest::GetNumCommittees { epoch, reply } => {
                handle(reply, self.inner.get_num_committees(epoch), context)
            },
            EpochManagerRequest::GetCommitteeForShardGroup {
                epoch,
                shard_group,
                limit,
                reply,
            } => handle(
                reply,
                self.inner
                    .get_committee_for_shard_group(epoch, shard_group, true, limit),
                context,
            ),
            EpochManagerRequest::GetCommitteesOverlappingShardGroup {
                epoch,
                shard_group,
                reply,
            } => handle(
                reply,
                self.inner.get_committees_overlapping_shard_group(epoch, shard_group),
                context,
            ),
            EpochManagerRequest::GetFeeClaimPublicKey { reply } => {
                handle(reply, self.inner.get_fee_claim_public_key(), context)
            },

            EpochManagerRequest::AddIntentToEvictValidator { proof, reply } => {
                handle(reply, self.inner.add_intent_to_evict_validator(*proof).await, context)
            },
            EpochManagerRequest::GetRandomCommitteeMemberFromShardGroup {
                epoch,
                shard_group,
                excluding,
                reply,
            } => handle(
                reply,
                self.inner
                    .get_random_committee_member_from_shard_group(epoch, shard_group, excluding),
                context,
            ),
            EpochManagerRequest::GetNetworkDescription { reply } => {
                handle(reply, self.inner.get_network_description(), context)
            },
        }
    }
}

fn handle<T>(
    reply: oneshot::Sender<Result<T, EpochManagerError>>,
    result: Result<T, EpochManagerError>,
    context: &str,
) {
    if let Err(ref e) = result {
        // These responses are not errors
        if !e.is_not_registered_error() && !e.is_not_found_error() {
            error!(target: LOG_TARGET, "Request {} failed with error: {}", context, e);
        }
    }
    if reply.send(result).is_err() {
        error!(target: LOG_TARGET, "Requester abandoned request");
    }
}
