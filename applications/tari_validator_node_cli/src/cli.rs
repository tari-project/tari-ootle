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

use std::path::PathBuf;

use clap::Parser;
use multiaddr::Multiaddr;

use crate::command::Command;

/// Tari Validator Node CLI
///
/// A command-line interface for interacting with the Tari validator node daemon.
/// This tool allows you to manage templates, keys, transactions, accounts, manifests, and peers.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Cli {
    /// JSON-RPC endpoint for the validator node daemon
    ///
    /// Specifies the network address where the validator node daemon is listening.
    /// Defaults to /ip4/127.0.0.1/tcp/18200 if not specified.
    /// Can also be set via the JRPC_ENDPOINT environment variable.
    ///
    /// Example: /ip4/127.0.0.1/tcp/18200
    #[clap(long, short = 'e', alias = "endpoint", env = "JRPC_ENDPOINT")]
    pub vn_daemon_jrpc_endpoint: Option<Multiaddr>,

    /// Base directory for storing CLI data
    ///
    /// Specifies where keys and other CLI data are stored.
    /// Defaults to ~/.tari/vncli if not specified.
    ///
    /// Example: /path/to/custom/directory
    #[clap(long, short = 'b', alias = "basedir")]
    pub base_dir: Option<PathBuf>,

    #[clap(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn init() -> Self {
        Self::parse()
    }
}
