//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

pub type WasmAbiVersion = u16;
/// The version that will be compiled into WASM templates (those using template_macros).
pub const LATEST_TEMPLATE_VERSION: WasmAbiVersion = 0;
/// The minimum supported version ABI version
pub const MINIMUM_SUPPORTED_WASM_ABI_VERSION: WasmAbiVersion = 0;
