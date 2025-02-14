//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod config;
mod epoch_manager;
mod epoch_manager_service;
mod error;
mod handle;
mod initializer;
mod types;

pub use config::EpochManagerConfig;
pub use epoch_manager::EpochManager;
pub use handle::EpochManagerHandle;
pub use initializer::spawn_service;
pub use types::*;
