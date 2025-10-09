//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use futures::TryStreamExt;
use tari_indexer_client::{rest_api_client::IndexerRestApiClient, types::GetUtxoUpdatesRequest};
use tari_ootle_common_types::{NumPreshards, StateVersion};

#[tokio::test]
#[ignore = "Requires a running indexer listening on a specific port"]
async fn dev_test() {
    let mut client = IndexerRestApiClient::connect("http://localhost:12017").unwrap();
    let mut stream = client
        .stream_utxo_updates_protobuf(GetUtxoUpdatesRequest {
            shard_state_versions: NumPreshards::current()
                .all_shards_iter()
                .map(|shard| (shard, StateVersion::zero()))
                .collect(),
            resource_address: "resource_0101010101010101010101010101010101010101010101010101010101010101"
                .parse()
                .unwrap(),
            unspent_only: false,
            per_shard_limit: 1000,
        })
        .await
        .unwrap();

    let mut count = 0usize;
    while let Some(msg) = stream.try_next().await.unwrap() {
        count += 1;
        eprintln!("{count} {:?} {:?} {:?}", msg.sos, msg.update, msg.eos);
    }
}
