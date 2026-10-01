// Copyright 2022 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

mod error;
pub use error::*;

mod environment;

mod module;
pub use module::{LoadedWasmTemplate, WasmModule};

#[cfg(feature = "wasm-cache")]
mod cache;
#[cfg(feature = "wasm-cache")]
pub use cache::{
    DiskCachedWasmTemplateProvider,
    DiskCachedWasmTemplateProviderError,
    ENGINE_FINGERPRINT,
    WasmModuleCache,
};

mod bulk_metering;
mod engine;
mod metering;
mod module_shape;
mod process;

pub use module_shape::ModuleStructure;
pub use process::WasmProcess;

mod instance_cache;
mod instance_reset;
pub use instance_cache::{Checkout, WasmInstance, WasmInstanceCache};
pub use instance_reset::{InitialState, InstanceResetError};

mod limiting_tunable;
mod mem_writer;
mod memory_pool;
pub use memory_pool::{MemoryPool, PooledMemoryTunables};
