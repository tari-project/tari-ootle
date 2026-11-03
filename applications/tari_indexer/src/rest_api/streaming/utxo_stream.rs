//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use async_stream::try_stream;
use axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use log::*;
use tari_indexer_client::{
    protobuf,
    types::{GetUtxoUpdatesRequest, UtxoStateUpdateSet, WalletUtxoUpdate},
};
use tari_ootle_common_types::{StateVersion, optional::Optional, shard::Shard};

use crate::{
    rest_api::{encoder::Encoder, error::ErrorResponse, streaming::encoding::MimeTypeEncoder},
    storage_sqlite::SqliteIndexerStore,
    store::ReadOnlyStore,
};

const LOG_TARGET: &str = "tari::indexer::rest_api::streaming::utxo_stream";

pub struct UtxoUpdateStream {
    content_type: HeaderValue,
    inner: Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>,
}

impl UtxoUpdateStream {
    pub fn new(
        store: ReadOnlyStore<SqliteIndexerStore>,
        request: GetUtxoUpdatesRequest,
        encoder: MimeTypeEncoder,
    ) -> Self {
        let content_type = encoder.media_type();
        let inner = try_stream! {
            let mut buffer = BytesMut::with_capacity(1024);
            // Read once before the shard reads. A verified version only rises, and commits with the rows it covers,
            // so one read earlier than a shard's rows is still covered by them.
            let sync_progress = store.get_sync_progress().await.optional().map_err(anyhow::Error::from)?;
            for &(shard, state_version) in &request.shard_state_versions {
                debug!(
                    target: LOG_TARGET,
                    "Requesting UTXO updates for shard {} from state version {}",
                    shard,
                    state_version
                );

                let (UtxoStateUpdateSet {
                    updates,
                    max_state_version,
                    max_epoch,
                    has_more,
                }, tombstone_horizon) = store
                    .get_utxo_updates(
                        request.resource_address,
                        request.from_epoch,
                        shard,
                        state_version,
                        request.unspent_only,
                        request.per_shard_limit,
                        sync_progress.as_ref().and_then(|progress| progress.verified_version(shard)),
                    )
                    .await
                    .map_err(anyhow::Error::from)?;

                // `max_state_version` is the version the read reached, and becomes the client's
                // resume point for the shard. Every update at or below it was either delivered here
                // or excluded by this request's `from_epoch`/`unspent_only` filters. A shard whose
                // read matched nothing still reports it when the read moved past the cursor, so that
                // the client does not read the excluded rows again on its next pass.
                //
                // A cursor below the tombstone horizon is told its spends are incomplete whatever the read
                // found, so a shard with nothing past the cursor still sends its frame.
                let spends_incomplete = spends_incomplete(request.unspent_only, state_version, tombstone_horizon);
                if updates.is_empty() && max_state_version <= state_version && !spends_incomplete {
                    debug!(target: LOG_TARGET, "No updates found for shard {}, moving to next shard", shard);
                    continue;
                }

                debug!(
                    target: LOG_TARGET,
                    "Received {} updates for shard {shard}, max_epoch = {max_epoch}, max_state_version {max_state_version} (has_more = {has_more})",
                    updates.len(),
                );

                let mut pending = PendingUpdates {
                    sos_emitted: false,
                    shard,
                    max_state_version,
                    has_more,
                    spends_incomplete,
                    updates,
                    index: 0,
                };

                loop {
                    let mut payload = protobuf::UtxoUpdatePayload::default();

                    if !pending.sos_emitted {
                        payload.sos = Some(protobuf::StartOfShard {
                            shard: pending.shard.as_u32(),
                            max_state_version: pending.max_state_version.as_u64(),
                            num_updates: u32::try_from(pending.updates_len()).expect("loaded more than u32::MAX updates"),
                            has_more: pending.has_more,
                            spends_incomplete: pending.spends_incomplete,
                        });
                        pending.sos_emitted = true;
                    }

                    if let Some(update) = pending.next() {
                        payload.update = Some(wallet_utxo_update_to_protobuf(update));
                    }

                    if pending.is_empty() {
                        payload.eos = Some(protobuf::EndOfShard {
                            max_state_version: pending.max_state_version.as_u64(),
                        });
                    }

                    encoder.encode_into(&payload, &mut buffer)?;

                    debug!(
                        target: LOG_TARGET,
                        "Encoded payload size: {}, pending updates remaining in batch: {}",
                        buffer.len(),
                        pending.updates_len()
                    );

                    yield buffer.split().freeze();

                    if payload.eos.is_some() {
                        break;
                    }
                }
            }
        };

        Self {
            content_type,
            inner: Box::pin(inner),
        }
    }
}

impl Stream for UtxoUpdateStream {
    type Item = anyhow::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}

impl IntoResponse for UtxoUpdateStream {
    fn into_response(self) -> Response {
        let content_type = self.content_type.clone();
        let stream_body = axum::body::Body::from_stream(self);

        axum::response::Response::builder()
            .header(header::CONTENT_TYPE, content_type)
            .body(stream_body)
            .map_err(ErrorResponse::anyhow)
            .into_response()
    }
}

struct PendingUpdates {
    pub sos_emitted: bool,
    pub shard: Shard,
    pub max_state_version: StateVersion,
    pub has_more: bool,
    pub spends_incomplete: bool,
    pub updates: Vec<WalletUtxoUpdate>,
    pub index: usize,
}

impl PendingUpdates {
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

/// Whether a read of a shard from `cursor` may be missing spends, which it is when it asks for them and resumes below
/// the shard's tombstone horizon. A cursor of zero is a full scan, and misses them as much as any other.
fn spends_incomplete(unspent_only: bool, cursor: StateVersion, tombstone_horizon: Option<StateVersion>) -> bool {
    !unspent_only && tombstone_horizon.is_some_and(|horizon| cursor < horizon)
}

fn wallet_utxo_update_to_protobuf(update: &WalletUtxoUpdate) -> protobuf::WalletUtxoUpdate {
    match update {
        WalletUtxoUpdate::Unspent(unspent) => protobuf::WalletUtxoUpdate::Unspent(protobuf::UtxoUnspent {
            tag: unspent.tag.value(),
            public_nonce: unspent.public_nonce.to_vec(),
        }),
        WalletUtxoUpdate::Spent(spent) => protobuf::WalletUtxoUpdate::Spent(protobuf::UtxoSpent {
            id: spent.id.as_bytes().to_vec(),
            version: spent.version.as_u64(),
        }),
        WalletUtxoUpdate::Burnt(burnt) => protobuf::WalletUtxoUpdate::Burnt(protobuf::UtxoBurnt {
            id: burnt.id.as_bytes().to_vec(),
            version: burnt.version.as_u64(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::Epoch;

    use super::*;
    use crate::{
        rest_api::streaming::encoding::JsonEncoder,
        storage_sqlite::tests::{store_with_utxos, utxo_resource},
        store::{IndexerStore, IndexerStoreWriteTransaction},
    };

    /// The start-of-shard frames a stream from `cursors` sends.
    async fn stream_frames(
        store: &SqliteIndexerStore,
        cursors: &[(u32, u64)],
        unspent_only: bool,
    ) -> Vec<protobuf::StartOfShard> {
        let request = GetUtxoUpdatesRequest {
            from_epoch: Epoch(0),
            shard_state_versions: cursors
                .iter()
                .map(|&(shard, version)| (Shard::from(shard), StateVersion::new(version)))
                .collect(),
            resource_address: utxo_resource(),
            unspent_only,
            per_shard_limit: 10,
        };
        let stream = UtxoUpdateStream::new(
            ReadOnlyStore::new(store.clone()),
            request,
            MimeTypeEncoder::Json(JsonEncoder),
        );
        let chunks = stream.map(|chunk| chunk.unwrap()).collect::<Vec<_>>().await;
        chunks
            .iter()
            .filter_map(|chunk| {
                serde_json::from_slice::<protobuf::UtxoUpdatePayload>(chunk)
                    .unwrap()
                    .sos
            })
            .collect()
    }

    /// The shard's head fetch reached v1000 on other substates, far past the resource's newest row at v10. A read
    /// from below the horizon is told its spends are incomplete even with nothing past its cursor, and a read that
    /// reaches the end of the shard resumes from the horizon, which clears the flag.
    #[tokio::test]
    async fn a_read_below_the_tombstone_horizon_resumes_from_above_it() {
        let (_dir, store) = store_with_utxos(&[(10, 1)]).await;
        store
            .with_write_tx(|tx| tx.raise_utxo_tombstone_horizon(Shard::from(1u32), StateVersion::new(1000)))
            .await
            .unwrap();

        let frames = stream_frames(&store, &[(1, 10)], false).await;
        assert_eq!(frames.len(), 1);
        assert!(frames[0].spends_incomplete);
        assert_eq!(frames[0].num_updates, 0);

        let frames = stream_frames(&store, &[(1, 0), (2, 0)], true).await;
        assert_eq!(frames.len(), 1, "the empty shard without a horizon sends nothing");
        assert!(!frames[0].spends_incomplete);
        assert_eq!(frames[0].num_updates, 1);
        assert_eq!(frames[0].max_state_version, 1000);

        let frames = stream_frames(&store, &[(1, 1000)], false).await;
        assert!(frames.is_empty(), "{frames:?}");
    }

    /// Another resource's spend in the shard, newer than every row of the client's resource, ages out. A client that
    /// read to the end of its resource's rows resumes from the shard's verified version, above that spend, so pruning
    /// it does not flag the client.
    #[tokio::test]
    async fn pruning_another_resources_tombstone_does_not_flag_a_caught_up_client() {
        use std::str::FromStr;

        use tari_ootle_common_types::SubstateVersion;
        use tari_state_tree::TransitionAccumulator;
        use tari_template_lib_types::{ResourceAddress, UtxoAddress, UtxoId};

        use crate::{
            network_state_sync::{SyncProgress, VerifiedShardVersion},
            storage_sqlite::models::{Key, UtxoSpent, UtxoUpdateRecord},
        };

        let shard = Shard::from(1u32);
        let (_dir, store) = store_with_utxos(&[(10, 1)]).await;
        let other_resource =
            ResourceAddress::from_str("resource_0101010101010101010101010101010101010101010101010101010101010101")
                .unwrap();
        let other_spend = UtxoUpdateRecord::Spent(UtxoSpent {
            address: UtxoAddress::new(other_resource, UtxoId::from_array([9; 32])),
            version: SubstateVersion::ZERO,
            shard,
            state_version: StateVersion::new(50),
        });
        let mut progress = SyncProgress::default();
        progress.record_verified(shard, VerifiedShardVersion {
            state_version: StateVersion::new(50),
            epoch: Epoch(5),
            transition_acc: TransitionAccumulator::zero(),
        });
        store
            .with_write_tx(move |tx| {
                tx.batch_insert_utxo_updates(Epoch(5), [other_spend])?;
                tx.key_value_set(Key::SyncProgress, progress)
            })
            .await
            .unwrap();

        let frames = stream_frames(&store, &[(1, 0)], false).await;
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].num_updates, 1);
        assert_eq!(
            frames[0].max_state_version, 50,
            "the read resumes from the verified version"
        );

        let num_pruned = store
            .with_write_tx(|tx| tx.prune_utxo_tombstones_before_epoch(Epoch(10), 100))
            .await
            .unwrap();
        assert_eq!(num_pruned, 1);
        let frames = stream_frames(&store, &[(1, 50)], false).await;
        assert!(frames.is_empty(), "the caught-up client is not flagged: {frames:?}");
    }

    #[test]
    fn a_cursor_below_the_tombstone_horizon_reads_incomplete_spends() {
        let v = StateVersion::new;

        assert!(spends_incomplete(false, v(0), Some(v(10))));
        assert!(spends_incomplete(false, v(9), Some(v(10))));
        assert!(!spends_incomplete(false, v(10), Some(v(10))));
        assert!(!spends_incomplete(false, v(0), None));
        assert!(!spends_incomplete(true, v(9), Some(v(10))));
    }
}
