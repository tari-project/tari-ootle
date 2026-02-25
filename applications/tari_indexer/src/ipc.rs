//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use libp2p::{Multiaddr, swarm::dial_opts::PeerCondition};
use ootle_byte_type::FromByteType;
use tari_networking::{DialOpts, NetworkingService, is_supported_multiaddr};
use tari_ootle_p2p::public_key_to_peer_id;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::bootstrap::Services;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum IpcMessage {
    AddPeer(IpcAddPeer),
}

impl From<IpcAddPeer> for IpcMessage {
    fn from(value: IpcAddPeer) -> Self {
        Self::AddPeer(value)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpcAddPeer {
    pub public_key: RistrettoPublicKeyBytes,
    pub addresses: Vec<Multiaddr>,
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("Deserialization error: {details}")]
    DeserializationError { details: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn handle_ipc_message(services: &Services, message: IpcMessage) -> Result<(), IpcError> {
    match message {
        IpcMessage::AddPeer(IpcAddPeer { public_key, addresses }) => {
            if let Some(a) = addresses.iter().find(|a| !is_supported_multiaddr(a)) {
                return Err(anyhow!("Unsupported multiaddr: {}", a).into());
            }

            let mut networking = services.networking.clone();
            let public_key = public_key
                .try_from_byte_type()
                .map_err(|_| anyhow!("Public key is malformed"))?;
            let peer_id = public_key_to_peer_id(public_key);

            let _dial_wait = networking
                .dial_peer(
                    DialOpts::peer_id(peer_id)
                        .addresses(addresses)
                        .condition(PeerCondition::Always)
                        .build(),
                )
                .await
                .map_err(|err| anyhow!("{}", err))?;

            Ok(())
        },
    }
}
