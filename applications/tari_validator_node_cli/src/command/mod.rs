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

mod key;
pub use key::KeysSubcommand;

mod template;
pub use template::TemplateSubcommand;

use crate::command::{
    account::AccountsSubcommand,
    manifest::ManifestSubcommand,
    peer::PeersSubcommand,
    transaction::TransactionSubcommand,
};

mod manifest;

mod account;
mod peer;
pub mod transaction;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand, Clone)]
pub enum Command {
    /// Manage and query templates
    ///
    /// Templates define the code and structure for smart contracts on the Tari network.
    /// Use this command to retrieve template information including ABI details.
    #[clap(subcommand, alias = "template")]
    Templates(TemplateSubcommand),

    /// Manage cryptographic key pairs
    ///
    /// Key pairs are used to sign transactions. You can create new keys,
    /// list existing keys, and switch between different key pairs.
    #[clap(subcommand, alias = "key")]
    Keys(KeysSubcommand),

    /// Submit and query transactions
    ///
    /// Transactions execute instructions on the Tari network. Use this command
    /// to submit new transactions or check the status of existing ones.
    #[clap(subcommand, alias = "transaction")]
    Transactions(TransactionSubcommand),

    /// Manage accounts
    ///
    /// Accounts are components that own resources and can execute transactions.
    /// Use this command to create and manage accounts on the network.
    #[clap(subcommand, alias = "accounts")]
    Accounts(AccountsSubcommand),

    /// Work with transaction manifests
    ///
    /// Manifests are human-readable transaction descriptions that can be
    /// compiled into executable transactions. Create and validate manifests here.
    #[clap(subcommand, alias = "manifest")]
    Manifests(ManifestSubcommand),

    /// Manage network peer connections
    ///
    /// Connect to other validator nodes on the network to enable
    /// communication and consensus participation.
    #[clap(subcommand, alias = "peer")]
    Peers(PeersSubcommand),
}
