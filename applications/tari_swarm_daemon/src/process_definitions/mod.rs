//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod context;
mod definition;
mod indexer;
mod minotari_miner;
mod minotari_node;
mod minotari_wallet;
mod validator_node;
mod wallet_daemon;
mod wallet_daemon_create_key;

pub use context::*;
pub use definition::*;
pub use wallet_daemon::{WALLET_DAEMON_AUTH_SETTINGS_KEY, WALLET_DAEMON_SEED_WORDS_SETTINGS_KEY};

use crate::config::InstanceType;

pub fn get_definition(instance_type: InstanceType) -> Box<dyn ProcessDefinition + 'static> {
    match instance_type {
        InstanceType::MinoTariNode => Box::new(minotari_node::MinotariNode::new()),
        InstanceType::MinoTariConsoleWallet => Box::new(minotari_wallet::MinotariWallet::new()),
        InstanceType::MinoTariMiner => Box::new(minotari_miner::MinotariMiner::new()),
        InstanceType::TariValidatorNode => Box::new(validator_node::ValidatorNode::new()),
        InstanceType::TariWalletDaemon => Box::new(wallet_daemon::WalletDaemon::new()),
        InstanceType::TariIndexer => Box::new(indexer::Indexer::new()),
        InstanceType::TariWalletDaemonCreateKey => Box::new(wallet_daemon_create_key::WalletDaemonCreateAccount::new()),
    }
}

pub const ARGS_SETTINGS_KEY: &str = "args";
