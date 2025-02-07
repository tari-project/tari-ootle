//   Copyright 2023. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    collections::HashMap,
    fmt::Display,
    future::poll_fn,
    task::{Context, Poll, Waker},
};

use futures::future::BoxFuture;
use log::*;
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{displayable::Displayable, NodeAddressable, ToPeerId};
use tari_dan_storage::global::DbTemplateType;
use tari_engine_types::hashing::hash_template_code;
use tari_epoch_manager::{base_layer::EpochManagerHandle, EpochManagerReader};
use tari_template_lib::models::TemplateAddress;
use tari_validator_node_rpc::client::TariValidatorNodeRpcClientFactory;

use crate::{
    implementation::template_sync_task::{TemplateSyncClientTask, TemplateSyncError},
    interface::TemplateManagerError,
};

const LOG_TARGET: &str = "tari::dan::template_manager::sync_worker";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TemplateSyncRequest {
    pub address: TemplateAddress,
    pub expected_binary_hash: FixedHash,
}

#[derive(Debug)]
pub enum SyncWorkerEvent {
    SyncRoundCompleted {
        result: TemplateBatchSyncResult,
    },
    SyncError {
        error: TemplateManagerError,
        batch: Vec<TemplateSyncRequest>,
    },
}

impl Display for SyncWorkerEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SyncRoundCompleted { result } => {
                write!(
                    f,
                    "SyncRoundCompleted {{ {} synced, {} unfulfilled, {} failed, abort: {} }}",
                    result.synced.len(),
                    result.unfulfilled.len(),
                    result.failed.len(),
                    result.sync_aborted.display()
                )
            },
            Self::SyncError { error, batch } => {
                write!(f, "SyncError {{ error: {}, batch: {} }}", error, batch.len())
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Services<TAddr> {
    epoch_manager: EpochManagerHandle<TAddr>,
    client_factory: TariValidatorNodeRpcClientFactory,
}

type SyncTaskResult = Result<TemplateBatchSyncResult, (TemplateManagerError, Vec<TemplateSyncRequest>)>;

pub(super) struct TemplateSyncWorker<TAddr> {
    pending_sync: Option<BoxFuture<'static, SyncTaskResult>>,
    request_queue: Vec<TemplateSyncRequest>,
    waker: Option<Waker>,
    services: Services<TAddr>,
}

impl<TAddr: NodeAddressable + ToPeerId + 'static> TemplateSyncWorker<TAddr> {
    pub fn new(epoch_manager: EpochManagerHandle<TAddr>, client_factory: TariValidatorNodeRpcClientFactory) -> Self {
        Self {
            pending_sync: None,
            waker: None,
            request_queue: Vec::new(),
            services: Services {
                epoch_manager,
                client_factory,
            },
        }
    }

    pub fn enqueue_all<I: IntoIterator<Item = TemplateSyncRequest>>(&mut self, requests: I) {
        let mut requests = requests.into_iter().peekable();
        if requests.peek().is_none() {
            return;
        }
        self.request_queue.extend(requests);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    /// Returns events for the sync worker. This must be polled to make progress on syncing.
    /// The future returned from this method is cancel-safe (can be used in a tokio::select! branch)
    pub async fn next(&mut self) -> SyncWorkerEvent {
        poll_fn(|cx| self.poll_next(cx)).await
    }

    fn poll_next(&mut self, cx: &mut Context) -> Poll<SyncWorkerEvent> {
        loop {
            // Work on syncing item
            if let Some(mut pending) = self.pending_sync.take() {
                match pending.as_mut().poll(cx) {
                    Poll::Ready(Ok(result)) => return Poll::Ready(SyncWorkerEvent::SyncRoundCompleted { result }),
                    Poll::Ready(Err((error, batch))) => {
                        return Poll::Ready(SyncWorkerEvent::SyncError { error, batch })
                    },
                    Poll::Pending => {
                        self.pending_sync = Some(pending);
                        // pending sync can wake
                        return Poll::Pending;
                    },
                }
            }

            if let Some(batch) = self.next_batch() {
                self.pending_sync = Some(Box::pin(do_sync(self.services.clone(), batch)));
            }

            if self.pending_sync.is_none() {
                self.waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }
    }

    fn next_batch(&mut self) -> Option<Vec<TemplateSyncRequest>> {
        if self.request_queue.is_empty() {
            return None;
        }
        const MAX_BATCH_SIZE: usize = 20;
        let n = self.request_queue.len().min(MAX_BATCH_SIZE);
        if n == 0 {
            return None;
        }
        let batch = self.request_queue.drain(0..n).collect();
        shrink_array(&mut self.request_queue);
        Some(batch)
    }
}

async fn do_sync<TAddr: NodeAddressable + ToPeerId>(
    services: Services<TAddr>,
    requests: Vec<TemplateSyncRequest>,
) -> SyncTaskResult {
    debug!(target: LOG_TARGET, "Starting next sync batch for {} template(s)", requests.len());
    match services.epoch_manager.current_epoch().await {
        Ok(current_epoch) => {
            let mut client_task = TemplateSyncClientTask::new(services.client_factory, services.epoch_manager);
            let result = client_task.run(requests, current_epoch).await;

            Ok(result)
        },
        Err(e) => Err((e.into(), requests)),
    }
}

#[derive(Debug, Default)]
pub(super) struct TemplateBatchSyncResult {
    pub synced: HashMap<TemplateAddress, SyncedTemplate>,
    pub unfulfilled: Vec<TemplateSyncRequest>,
    pub failed: Vec<(TemplateSyncRequest, TemplateSyncError)>,
    pub sync_aborted: Option<TemplateSyncError>,
}

#[derive(Debug)]
pub(super) struct SyncedTemplate {
    pub template_type: DbTemplateType,
    pub binary: Vec<u8>,
}

impl SyncedTemplate {
    pub fn hash_binary(&self) -> FixedHash {
        hash_template_code(self.binary.as_ref()).into_array().into()
    }
}

fn shrink_array<T>(vec: &mut Vec<T>) {
    const MAX_VEC_SHRINK_SIZE: usize = 500;
    let cap = vec.capacity();
    let len = vec.len();
    if len > MAX_VEC_SHRINK_SIZE {
        // Shrink once items are removed
        return;
    }
    if cap > MAX_VEC_SHRINK_SIZE {
        vec.shrink_to(MAX_VEC_SHRINK_SIZE);
    }
}
