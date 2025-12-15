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

use std::path::Path;

use clap::Subcommand;

use crate::key_manager::KeyManager;

#[derive(Debug, Subcommand, Clone)]
pub enum KeysSubcommand {
    /// Create a new cryptographic key pair
    ///
    /// Generates a new Ed25519 key pair for signing transactions.
    /// The key is stored in the base directory and can be activated using the 'use' command.
    /// A key is automatically created on first run if none exists.
    ///
    /// Example:
    ///   tari_validator_node_cli keys new
    #[clap(alias = "create")]
    New,

    /// List all stored key pairs
    ///
    /// Displays all key pairs stored in the base directory, showing their public keys
    /// and indicating which one is currently active. The active key is used to sign
    /// all transactions submitted through this CLI.
    ///
    /// Example:
    ///   tari_validator_node_cli keys list
    List,

    /// Set a key pair as active
    ///
    /// Changes the active key pair used for signing transactions.
    /// You must specify the public key (in hex format) of the key you want to activate.
    ///
    /// Arguments:
    ///   name - The public key (hex string) of the key pair to activate
    ///
    /// Example:
    ///   tari_validator_node_cli keys use 0x1234567890abcdef...
    Use {
        /// Public key in hexadecimal format
        name: String,
    },
}

impl KeysSubcommand {
    pub async fn handle<P: AsRef<Path>>(self, base_dir: P) -> anyhow::Result<()> {
        let key_manager = KeyManager::init(base_dir)?;

        #[allow(clippy::enum_glob_use)]
        use KeysSubcommand::*;
        match self {
            New => {
                let key = key_manager.create()?;
                println!("New key pair {} created", key);
            },
            List => {
                println!("Key pairs:");
                for (i, key) in key_manager.all().into_iter().enumerate() {
                    if key.is_active {
                        println!("{}. {} (active)", i, key);
                    } else {
                        println!("{}. {}", i, key);
                    }
                }
            },
            Use { name } => {
                key_manager.set_active_key(&name)?;
                println!("Key {} is now active", name);
            },
        }
        Ok(())
    }
}
