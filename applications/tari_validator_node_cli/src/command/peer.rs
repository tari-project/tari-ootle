//   Copyright 2022. The Tari Project
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

use clap::Subcommand;
use multiaddr::Multiaddr;
use tari_validator_node_client::{types::AddPeerRequest, ValidatorNodeClient};

#[derive(Debug, Subcommand, Clone)]
pub enum PeersSubcommand {
    /// Connect to a peer validator node
    ///
    /// Establishes a connection to another validator node on the network using its
    /// public key and network addresses. This enables communication for consensus,
    /// transaction propagation, and other network activities.
    ///
    /// The command will wait for the dial to complete before returning.
    ///
    /// Arguments:
    ///   public_key - The peer's public key in hexadecimal format
    ///   addresses - One or more multiaddr network addresses for reaching the peer
    ///
    /// Example:
    ///   tari_validator_node_cli peers connect <public_key> /ip4/192.168.1.100/tcp/18189 /ip4/192.168.1.100/tcp/18189
    Connect {
        /// Peer's public key in hexadecimal format
        public_key: String,
        /// Network addresses (multiaddr format) where the peer can be reached
        addresses: Vec<Multiaddr>,
    },
}

impl PeersSubcommand {
    pub async fn handle(self, mut client: ValidatorNodeClient) -> anyhow::Result<()> {
        #[allow(clippy::enum_glob_use)]
        use PeersSubcommand::*;
        match self {
            Connect { public_key, addresses } => {
                client
                    .add_peer(AddPeerRequest {
                        public_key: public_key.parse().map_err(anyhow::Error::msg)?,
                        addresses,
                        wait_for_dial: true,
                    })
                    .await?;
                println!("🫂 Peer connected");
            },
        }
        Ok(())
    }
}
