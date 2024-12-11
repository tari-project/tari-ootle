//  Copyright 2023. The Tari Project
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

use tari_base_node_client::grpc::GrpcBaseNodeClient;
use tari_common_types::types::PublicKey;
use tari_dan_common_types::{DerivableFromPublicKey, NodeAddressable};
use tari_dan_storage::global::GlobalDb;
use tari_dan_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_shutdown::ShutdownSignal;
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use crate::{
    base_layer::{config::EpochManagerConfig, epoch_manager_service::EpochManagerService, EpochManagerHandle},
    traits::LayerOneTransactionSubmitter,
};

pub fn spawn_service<TAddr, TLayerOneSubmitter>(
    config: EpochManagerConfig,
    global_db: GlobalDb<SqliteGlobalDbAdapter<TAddr>>,
    base_node_client: GrpcBaseNodeClient,
    node_public_key: PublicKey,
    layer_one_submitter: TLayerOneSubmitter,
    shutdown: ShutdownSignal,
) -> (EpochManagerHandle<TAddr>, JoinHandle<anyhow::Result<()>>)
where
    TAddr: NodeAddressable + DerivableFromPublicKey + 'static,
    TLayerOneSubmitter: LayerOneTransactionSubmitter + Send + Sync + 'static,
{
    let (tx_request, rx_request) = mpsc::channel(10);
    let (events, _) = broadcast::channel(100);
    let epoch_manager = EpochManagerHandle::new(tx_request, events.clone());
    let handle = EpochManagerService::spawn(
        config,
        events,
        rx_request,
        shutdown,
        global_db,
        base_node_client,
        layer_one_submitter,
        node_public_key,
    );
    (epoch_manager, handle)
}
