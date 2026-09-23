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
mod engine_config;
mod metering;
mod module_shape;
mod process;

pub use process::WasmProcess;

mod limiting_tunable;
mod mem_writer;
