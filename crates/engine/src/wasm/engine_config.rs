//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! The compiler configuration every cache artifact is built under.
//!
//! Its own file because [`ENGINE_FINGERPRINT`](super::cache::ENGINE_FINGERPRINT) hashes this
//! source: an artifact is only interchangeable with a fresh compile under the exact settings that
//! produced it, so every change here has to move the fingerprint. Keeping the configuration alone
//! in a file is what lets that be true without every edit to the loader orphaning the cache.

use std::sync::Arc;

use tari_engine_types::limits;
use wasmer::{
    Engine,
    Pages,
    sys::{BaseTunables, CompilerConfig, Cranelift, CraneliftOptLevel, EngineBuilder},
};

use crate::wasm::{bulk_metering::BulkMetering, limiting_tunable::LimitingTunables, metering};

/// Build the engine that compiles a template and that a cached artifact is deserialized against.
pub(crate) fn create_engine() -> Engine {
    const MEMORY_PAGE_LIMIT: Pages = Pages(limits::WASM_LIMITS.max_memory_pages as u32);
    let base = BaseTunables::new();
    let tunables = LimitingTunables::new(base, MEMORY_PAGE_LIMIT, limits::WASM_LIMITS.max_table_elements);
    let mut compiler = Cranelift::new();
    compiler
        .opt_level(CraneliftOptLevel::SpeedAndSize)
        .canonicalize_nans(true);
    // Per-call metering ceiling. `WasmProcess::invoke` lowers each call's allowance further to
    // whatever remains of the per-transaction budget (`MAX_WASM_POINTS_PER_TRANSACTION`).
    let metering = Arc::new(metering::middleware(limits::MAX_WASM_POINTS_PER_CALL));
    compiler.push_middleware(metering.clone());
    // Must follow the static meter: `BulkMetering` reads the global indexes that meter installs
    // and relies on its own emitted operators escaping static costing.
    compiler.push_middleware(Arc::new(BulkMetering::new(metering)));

    // Every feature is set explicitly rather than relying on `Features::default()`: the
    // accepted-module set is consensus-critical, and wasmer flips defaults between releases
    // (e.g. `extended_const` became default-on in 7.1.0). When bumping wasmer, add any newly
    // introduced feature flag here explicitly.
    let mut features = wasmer::sys::Features::default();
    features
        .threads(false)
        .bulk_memory(true)
        .multi_value(false)
        .reference_types(true)
        .simd(false)
        .relaxed_simd(false)
        .tail_call(false)
        .memory64(false)
        .multi_memory(false)
        .exceptions(false)
        .module_linking(false)
        .extended_const(false)
        .wide_arithmetic(false);

    let mut engine = EngineBuilder::new(compiler).set_features(Some(features)).engine();
    engine.set_tunables(tunables);
    Engine::from(engine)
}
