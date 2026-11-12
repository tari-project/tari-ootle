//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use libp2p::{
    Multiaddr,
    PeerId,
    futures,
    futures::{Stream, StreamExt, stream::BoxStream},
};

use crate::peer_record::SignedPeerRecord;
#[async_trait]
pub trait PeerStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error;
    type Stream: Stream<Item = Result<SignedPeerRecord, Self::Error>> + Unpin + Send;

    async fn get(&self, peer_id: &PeerId) -> Result<Option<SignedPeerRecord>, Self::Error>;
    async fn put(&self, peer: SignedPeerRecord) -> Result<(), Self::Error>;
    async fn put_if_newer(&self, peer: SignedPeerRecord) -> Result<(), Self::Error> {
        if let Some(existing) = self.get(&peer.to_peer_id()).await? &&
            existing.updated_at.as_secs() >= peer.updated_at.as_secs()
        {
            return Ok(());
        }
        self.put(peer).await
    }
    /// Stores `peer` unless the stored record has a later `updated_at`, returning whether it was stored. A record
    /// with the same `updated_at` replaces the stored one, because the wire format carries `updated_at` in whole
    /// seconds and a peer can re-sign its record more than once within a second.
    async fn put_unless_older(&self, peer: SignedPeerRecord) -> Result<bool, Self::Error> {
        if let Some(existing) = self.get(&peer.to_peer_id()).await? &&
            existing.updated_at.as_secs() > peer.updated_at.as_secs()
        {
            return Ok(false);
        }
        self.put(peer).await?;
        Ok(true)
    }
    async fn put_address(&self, peer_id: &PeerId, address: Multiaddr) -> Result<bool, Self::Error>;
    async fn remove(&self, peer_id: &PeerId) -> Result<Option<SignedPeerRecord>, Self::Error>;

    async fn difference<'a, I: IntoIterator<Item = &'a PeerId> + Send>(
        &self,
        peers: I,
    ) -> Result<HashSet<PeerId>, Self::Error>;
    fn stream(&self) -> Self::Stream;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryPeerStore {
    peers: Arc<RwLock<HashMap<PeerId, SignedPeerRecord>>>,
}

impl MemoryPeerStore {
    pub fn new() -> Self {
        Self {
            peers: Default::default(),
        }
    }
}

#[async_trait]
impl PeerStore for MemoryPeerStore {
    type Error = Infallible;
    type Stream = BoxStream<'static, Result<SignedPeerRecord, Self::Error>>;

    async fn get(&self, peer_id: &PeerId) -> Result<Option<SignedPeerRecord>, Self::Error> {
        Ok(self.peers.read().unwrap().get(peer_id).cloned())
    }

    async fn put(&self, peer: SignedPeerRecord) -> Result<(), Self::Error> {
        tracing::debug!("STORE: put: {:?}", peer);
        self.peers.write().unwrap().insert(peer.to_peer_id(), peer);
        Ok(())
    }

    async fn put_address(&self, peer_id: &PeerId, address: Multiaddr) -> Result<bool, Self::Error> {
        let mut peers = self.peers.write().unwrap();
        match peers.get_mut(peer_id) {
            Some(peer) => {
                if !peer.addresses.contains(&address) {
                    peer.addresses.push(address);
                }
                Ok(true)
            },
            None => Ok(false),
        }
    }

    async fn remove(&self, peer_id: &PeerId) -> Result<Option<SignedPeerRecord>, Self::Error> {
        Ok(self.peers.write().unwrap().remove(peer_id))
    }

    async fn difference<'a, I>(&self, peers: I) -> Result<HashSet<PeerId>, Self::Error>
    where I: IntoIterator<Item = &'a PeerId> + Send {
        let peers = peers.into_iter().copied().collect::<HashSet<_>>();
        Ok(peers
            .difference(&self.peers.read().unwrap().keys().copied().collect::<HashSet<_>>())
            .copied()
            .collect())
    }

    fn stream(&self) -> Self::Stream {
        futures::stream::iter(self.peers.read().unwrap().values().cloned().map(Ok).collect::<Vec<_>>()).boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use libp2p::identity;

    use super::*;
    use crate::peer_record::LocalPeerRecord;

    #[tokio::test]
    async fn put_unless_older_accepts_same_second_wire_record_after_in_memory_insert() {
        let store = MemoryPeerStore::new();
        let keypair = Arc::new(identity::Keypair::generate_ed25519());
        let mut local = LocalPeerRecord::new(keypair);
        let addr1: Multiaddr = "/ip4/127.0.0.1/tcp/8000".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.1/tcp/8001".parse().unwrap();

        local.add_address(addr1);
        let mut initial = SignedPeerRecord::from(local.clone());
        initial.updated_at += Duration::from_millis(500);
        store.put(initial).await.unwrap();

        local.add_address(addr2.clone());
        let encoded = local.encode_to_proto().unwrap();
        let decoded = SignedPeerRecord::decode_from_proto(&encoded).unwrap();

        assert!(store.put_unless_older(decoded).await.unwrap());
        let stored = store.get(&local.to_peer_id()).await.unwrap().unwrap();
        assert!(stored.addresses.contains(&addr2));
    }

    #[tokio::test]
    async fn put_address_deduplicates_existing_addresses() {
        let store = MemoryPeerStore::new();
        let keypair = Arc::new(identity::Keypair::generate_ed25519());
        let mut local = LocalPeerRecord::new(keypair);
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8000".parse().unwrap();
        local.add_address(addr.clone());

        let peer_id = local.to_peer_id();
        store.put(SignedPeerRecord::from(local)).await.unwrap();

        assert!(store.put_address(&peer_id, addr.clone()).await.unwrap());
        let stored = store.get(&peer_id).await.unwrap().unwrap();
        assert_eq!(stored.addresses, vec![addr]);
        assert!(stored.is_valid());
    }
}
