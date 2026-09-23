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

//! Derivation of the [`ModuleShape`] counts that price instantiation, and the structural limits
//! checked alongside them.
//!
//! Its own file because [`ENGINE_FINGERPRINT`](super::cache::ENGINE_FINGERPRINT) hashes this
//! source. A cache hit serves these counts verbatim out of the file header without re-deriving
//! them, and they price `instantiation_points` into a committed fee receipt, so a node that
//! counted differently when it wrote the file charges a different fee than one compiling fresh.
//! The limits enforced here are in the same position: `max_tables` and `max_globals` are checked
//! at compile time only, and a hit never reaches them.

use tari_engine_types::limits::{self, ModuleShape};
use wasmer::wasmparser::{DataKind, ElementItems, ElementKind, Parser, Payload};

use crate::wasm::WasmValidationError;

/// Checks what only the module bytes show: that the module declares no start function, and no more
/// tables or globals than the limits.
///
/// A start function runs on every instantiation, before the engine has installed this call's
/// metering allowance and outside any invocation it could attribute effects to. Templates have no
/// use for one: the engine only ever enters a template through its `<name>_main` export.
///
/// Tables and globals are both host storage built at every instantiation and claimed by a
/// declaration far smaller than what it claims. Each table's element count is bounded by the
/// tunables, which see one table at a time, so the number of tables is what bounds the storage all
/// of them together claim; a global's slot is fixed, so its count is the whole bound.
pub(crate) fn validate_module_structure(code: &[u8]) -> Result<ModuleShape, WasmValidationError> {
    let mut shape = ModuleShape::default();
    for payload in Parser::new(0).parse_all(code) {
        // Malformed wasm: stop and let the cranelift compile in
        // `load_template_from_code` report the canonical CompileError.
        let Ok(payload) = payload else { break };
        match payload {
            Payload::StartSection { .. } => return Err(WasmValidationError::StartSectionNotAllowed),
            Payload::TableSection(reader) => {
                let count = reader.count() as usize;
                if count > limits::WASM_LIMITS.max_tables {
                    return Err(WasmValidationError::TooManyTables {
                        count,
                        max_tables: limits::WASM_LIMITS.max_tables,
                    });
                }
                for table in reader.into_iter().flatten() {
                    shape.declared_table_slots = shape.declared_table_slots.saturating_add(table.ty.initial);
                }
            },
            Payload::GlobalSection(reader) => {
                let count = reader.count() as usize;
                if count > limits::WASM_LIMITS.max_globals {
                    return Err(WasmValidationError::TooManyGlobals {
                        count,
                        max_globals: limits::WASM_LIMITS.max_globals,
                    });
                }
            },
            // Only active segments are written into the instance's storage when it is built. A
            // passive segment stays in the module until a `memory.init` or `table.init` reaches for
            // it, so its bytes are a cost of that operator rather than of instantiation, and a call
            // that never executes one must not pay for it.
            Payload::DataSection(reader) => {
                for segment in reader.into_iter().flatten() {
                    if matches!(segment.kind, DataKind::Active { .. }) {
                        shape.data_segment_bytes = shape.data_segment_bytes.saturating_add(segment.data.len() as u64);
                        shape.data_segment_count = shape.data_segment_count.saturating_add(1);
                    }
                }
            },
            Payload::ElementSection(reader) => {
                for segment in reader.into_iter().flatten() {
                    if !matches!(segment.kind, ElementKind::Active { .. }) {
                        continue;
                    }
                    let entries = match segment.items {
                        ElementItems::Functions(items) => u64::from(items.count()),
                        ElementItems::Expressions(_, items) => u64::from(items.count()),
                    };
                    shape.element_segment_entries = shape.element_segment_entries.saturating_add(entries);
                }
            },
            _ => {},
        }
    }
    Ok(shape)
}
