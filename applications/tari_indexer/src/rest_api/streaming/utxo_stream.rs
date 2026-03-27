//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use axum::response::{IntoResponse, Response};
use bytes::{Bytes, BytesMut};
use futures::Stream;
use log::*;
use tari_indexer_client::{
    protobuf,
    types::{GetUtxoUpdatesRequest, UtxoStateUpdateSet, WalletUtxoUpdate},
};
use tari_ootle_common_types::{StateVersion, shard::Shard};

use crate::{
    rest_api::{encoder::Encoder, error::ErrorResponse, streaming::encoding::MimeTypeEncoder},
    substate_manager::SubstateManager,
};

const LOG_TARGET: &str = "tari::indexer::rest_api::streaming::utxo_stream";

pub struct UtxoUpdateStream {
    substate_manager: SubstateManager,
    request: GetUtxoUpdatesRequest,
    requested_shard_index: usize,
    buffer: BytesMut,
    pending_updates: Option<PendingUpdates>,
    is_done: bool,
    encoder: MimeTypeEncoder,
}

impl UtxoUpdateStream {
    pub fn new(substate_manager: SubstateManager, request: GetUtxoUpdatesRequest, encoder: MimeTypeEncoder) -> Self {
        Self {
            substate_manager,
            request,
            requested_shard_index: 0,
            pending_updates: None,
            buffer: BytesMut::with_capacity(1024),
            is_done: false,
            encoder,
        }
    }

    fn current_requested_state_shard(&self) -> Option<(Shard, StateVersion)> {
        self.request
            .shard_state_versions
            .get(self.requested_shard_index)
            .copied()
    }

    fn encode_to_buffer(&mut self) -> anyhow::Result<()> {
        let mut pending_updates = self
            .pending_updates
            .take()
            .expect("BUG: no pending updates in encode_to_buffer");

        let mut payload = protobuf::UtxoUpdatePayload::default();

        if !pending_updates.sos_emitted {
            payload.sos = Some(protobuf::StartOfShard {
                shard: pending_updates.shard.as_u32(),
                max_state_version: pending_updates.updates_state_version.as_u64(),
                num_updates: u32::try_from(pending_updates.updates_len()).expect("loaded more than u32::MAX updates"),
            });
            pending_updates.sos_emitted = true;
        }

        if let Some(update) = pending_updates.next() {
            payload.update = Some(wallet_utxo_update_to_protobuf(update));
        }

        if pending_updates.is_empty() {
            // No more updates in this batch
            payload.eos = Some(protobuf::EndOfShard {
                max_state_version: pending_updates.high_watermark_state_version.as_u64(),
            });
        }

        // This should be a max of about 60 bytes for protobuf (observed max 54 bytes)
        self.encoder.encode_into(&payload, &mut self.buffer)?;

        debug!(
            target: LOG_TARGET,
            "Encoded payload size: {}, pending updates remaining in batch: {}",
            self.buffer.len(),
            pending_updates.updates_len()
        );

        if payload.eos.is_none() {
            // Put pending updates back for next call
            self.pending_updates = Some(pending_updates);
        }

        Ok(())
    }

    pub fn next_batch(&mut self, shard: Shard, state_version: StateVersion) -> anyhow::Result<bool> {
        let UtxoStateUpdateSet {
            updates,
            max_state_version,
            max_epoch,
        } = self.substate_manager.get_utxo_updates(
            self.request.resource_address,
            self.request.from_epoch,
            shard,
            state_version,
            self.request.unspent_only,
            self.request.per_shard_limit,
        )?;
        let high_watermark_state_version = self
            .substate_manager
            .get_max_state_version(&self.request.resource_address, shard)?;
        if updates.is_empty() {
            return Ok(false);
        }
        debug!(
            target: LOG_TARGET,
            "Received {} updates for shard {shard}, max_epoch = {max_epoch}, max_state_version {max_state_version} -> {high_watermark_state_version}",
            updates.len(),
        );
        self.pending_updates = Some(PendingUpdates {
            sos_emitted: false,
            shard,
            updates_state_version: max_state_version,
            high_watermark_state_version,
            updates,
            index: 0,
        });

        Ok(true)
    }
}

impl Stream for UtxoUpdateStream {
    type Item = anyhow::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.is_done {
            return Poll::Ready(None);
        }

        let this = self.get_mut();
        loop {
            if this.pending_updates.is_none() {
                let (shard, state_version) = match this.current_requested_state_shard() {
                    Some(v) => v,
                    None => {
                        debug!(target: LOG_TARGET, "No more shards to request");
                        this.is_done = true;
                        return Poll::Ready(None);
                    },
                };

                debug!(
                    target: LOG_TARGET,
                    "Requesting UTXO updates for shard {} from state version {}",
                    shard,
                    state_version
                );

                // Get more updates
                match this.next_batch(shard, state_version) {
                    Ok(true) => {
                        // start sending updates
                    },
                    Ok(false) => {
                        // No updates in this shard, move to next
                        this.requested_shard_index += 1;
                        debug!(
                            target: LOG_TARGET,
                            "No updates found for shard {}, moving to next shard",
                            shard
                        );
                        continue;
                    },
                    Err(e) => {
                        error!(target: LOG_TARGET, "Error fetching UTXO updates: {}", e);
                        this.is_done = true;
                        break Poll::Ready(Some(Err(anyhow::anyhow!(e))));
                    },
                }
            }

            if let Err(e) = this.encode_to_buffer() {
                error!(target: LOG_TARGET, "Error encoding UTXO update: {}", e);
                this.is_done = true;
                return Poll::Ready(Some(Err(anyhow::anyhow!(e))));
            }

            debug!(
                target: LOG_TARGET,
                "Buffer size after encoding: {} bytes",
                this.buffer.len()
            );

            if this.pending_updates.is_none() {
                // Finished this shard, move to next
                // Note that the client has requested a maximum number of updates per shard, so we may not have sent all
                // updates, but it is up to the client to request again with updated state versions
                this.requested_shard_index += 1;
            }

            // Drain and send the buffer
            break Poll::Ready(Some(Ok(this.buffer.split().freeze())));
        }
    }
}

impl IntoResponse for UtxoUpdateStream {
    fn into_response(self) -> Response {
        let stream_body = axum::body::Body::from_stream(self);

        axum::response::Response::builder()
            .header("Content-Type", "application/octet-stream")
            .body(stream_body)
            .map_err(ErrorResponse::anyhow)
            .into_response()
    }
}

struct PendingUpdates {
    pub sos_emitted: bool,
    pub shard: Shard,
    pub updates_state_version: StateVersion,
    pub high_watermark_state_version: StateVersion,
    pub updates: Vec<WalletUtxoUpdate>,
    pub index: usize,
}

impl PendingUpdates {
    #[allow(dead_code)]
    pub fn is_done(&self) -> bool {
        self.index >= self.updates.len()
    }

    pub fn next(&mut self) -> Option<&WalletUtxoUpdate> {
        if self.is_done() {
            return None;
        }
        let update = &self.updates[self.index];
        self.index += 1;
        Some(update)
    }

    pub const fn updates_len(&self) -> usize {
        self.updates.len() - self.index
    }

    pub const fn is_empty(&self) -> bool {
        self.updates_len() == 0
    }
}

fn wallet_utxo_update_to_protobuf(update: &WalletUtxoUpdate) -> protobuf::WalletUtxoUpdate {
    match update {
        WalletUtxoUpdate::Unspent(unspent) => protobuf::WalletUtxoUpdate::Unspent(protobuf::UtxoUnspent {
            tag: unspent.tag.value(),
            public_nonce: unspent.public_nonce.to_vec(),
        }),
        WalletUtxoUpdate::Spent(spent) => protobuf::WalletUtxoUpdate::Spent(protobuf::UtxoSpent {
            id: spent.id.as_bytes().to_vec(),
            version: spent.version,
        }),
        WalletUtxoUpdate::Burnt(burnt) => protobuf::WalletUtxoUpdate::Burnt(protobuf::UtxoBurnt {
            id: burnt.id.as_bytes().to_vec(),
            version: burnt.version,
        }),
    }
}
