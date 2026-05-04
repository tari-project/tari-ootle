//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Static extraction of a template's `TemplateDef` directly from WASM bytes,
//! without invoking cranelift.
//!
//! Templates compiled from the `tari_template_lib` macros embed their ABI
//! definition as a tari-bor-encoded blob in the module's data section,
//! addressed by an exported global named [`ABI_TEMPLATE_DEF_GLOBAL_NAME`].
//! `WasmEnv::load_template_def` recovers it via wasmer instantiation and
//! linear-memory access; `extract_template_def` here recovers the same
//! bytes by parsing the WASM binary statically (no compile, no JIT, no
//! linear memory).
//!
//! Intended exclusively for callers that only need the type/function
//! metadata (today: only the wallet daemon's template monitor, which
//! records each authored template's ABI) and want to avoid the cranelift
//! compile cost. It does **not** validate the WASM module — anyone using
//! a template for execution should go through
//! `WasmModule::load_template_from_code`.
//!
//! # Toolchain assumptions
//!
//! The data-segment layout this extractor walks is what `rustc`'s
//! `wasm32-unknown-unknown` LLVM backend produces in practice for templates
//! built from `tari_template_lib`'s `#[template]` macro: a single active
//! data segment in memory 0 covering the static rodata, with the
//! `_ABI_TEMPLATE_DEF` global initialised to an `i32.const` pointer into
//! that segment, and the `[u32 LE: length] || [bor bytes]` blob laid out
//! contiguously at that pointer.
//!
//! Specifically, [`read_data_at`] requires the `[length, payload]` blob to
//! lie **entirely within a single active data segment**. Other WASM
//! toolchains (AssemblyScript, emscripten, custom code generators) may
//! split static data across multiple segments or place the rodata at
//! arbitrary offsets, in which case extraction fails with
//! [`ExtractTemplateDefError::DataOutOfRange`]. This is acceptable today
//! because the only consumer is the wallet daemon, and templates are only
//! built with `tari_template_lib`. If templates from arbitrary toolchains
//! ever need to be supported, the canonical fix is to switch the ABI
//! storage from "data segment + global" to a WASM **custom section** —
//! `Payload::CustomSection` is trivially extractable regardless of layout
//! and is the standard way (wasm-bindgen, DWARF, the `name` section, …)
//! to embed metadata in a WASM module.

use tari_template_abi::{ABI_TEMPLATE_DEF_GLOBAL_NAME, TemplateDef, WASM_PTR_SIZE};
use wasmer::wasmparser::{BinaryReaderError, Data, DataKind, ExternalKind, Operator, Parser, Payload, TypeRef};

/// Statically extract the embedded `TemplateDef` from a template's WASM bytes.
///
/// Cheap relative to a full compile: parses only the export, global, import
/// and data sections needed to locate and read the ABI blob. No cranelift, no
/// instantiation, no linear-memory allocation.
///
/// All arithmetic on offsets / lengths is checked. Adversarial inputs are
/// rejected with an explicit error rather than panicking or wrapping.
pub fn extract_template_def(code: &[u8]) -> Result<TemplateDef, ExtractTemplateDefError> {
    let mut imported_global_count: u32 = 0;
    let mut globals_init: Vec<Option<i32>> = Vec::new();
    let mut export_global_idx: Option<u32> = None;
    let mut data_segments = Vec::new();

    for payload in Parser::new(0).parse_all(code) {
        match payload? {
            Payload::ImportSection(reader) => {
                for imports in reader {
                    for entry in imports? {
                        let (_, import) = entry?;
                        if matches!(import.ty, TypeRef::Global(_)) {
                            imported_global_count = imported_global_count
                                .checked_add(1)
                                .ok_or(ExtractTemplateDefError::TooManyImportedGlobals)?;
                        }
                    }
                }
            },
            Payload::GlobalSection(reader) => {
                for global in reader {
                    let global = global?;
                    let mut ops = global.init_expr.get_operators_reader();
                    let init = match ops.read() {
                        Ok(Operator::I32Const { value }) => Some(value),
                        _ => None,
                    };
                    globals_init.push(init);
                }
            },
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    if export.kind == ExternalKind::Global && export.name == ABI_TEMPLATE_DEF_GLOBAL_NAME {
                        export_global_idx = Some(export.index);
                        break;
                    }
                }
            },
            Payload::DataSection(reader) => {
                for segment in reader {
                    let segment = segment?;
                    if let DataKind::Active {
                        memory_index: 0,
                        offset_expr,
                    } = &segment.kind
                    {
                        let mut ops = offset_expr.get_operators_reader();
                        if let Ok(Operator::I32Const { value }) = ops.read() {
                            data_segments.push((u64::from(value as u32), segment));
                        }
                    }
                }
            },
            _ => {},
        }
    }

    let global_idx = export_global_idx.ok_or(ExtractTemplateDefError::AbiExportMissing)?;
    if global_idx < imported_global_count {
        // The ABI global must be defined by the module itself, not imported.
        return Err(ExtractTemplateDefError::AbiExportImported);
    }
    let local_idx = (global_idx - imported_global_count) as usize;
    let abi_ptr = globals_init
        .get(local_idx)
        .copied()
        .flatten()
        .ok_or(ExtractTemplateDefError::AbiGlobalNotConst)?;
    let abi_ptr = u64::from(abi_ptr as u32);

    // [u32 LE: length] || [length bytes: tari-bor-encoded TemplateDef]
    let len_bytes = read_data_at(&data_segments, abi_ptr, WASM_PTR_SIZE)?;
    let length = u32::from_le_bytes(len_bytes.try_into().expect("WASM_PTR_SIZE == 4")) as usize;
    // Cap against the input binary size: an embedded TemplateDef cannot be
    // larger than the WASM file that contains it. Without this, a length
    // prefix of ~u32::MAX from adversarial bytes would force the search loop
    // in `read_data_at` to consider a 4 GiB span.
    if length > code.len() {
        return Err(ExtractTemplateDefError::AbiLengthExceedsBinary {
            length,
            binary_size: code.len(),
        });
    }

    let body_offset = abi_ptr
        .checked_add(WASM_PTR_SIZE as u64)
        .ok_or(ExtractTemplateDefError::OffsetOverflow)?;
    let body = read_data_at(&data_segments, body_offset, length)?;

    tari_bor::decode(body).map_err(ExtractTemplateDefError::Decode)
}

/// Returns a slice of length `len` starting at `offset` within the union of
/// the supplied active data segments (memory 0).
///
/// **Single-segment assumption.** The requested `[offset, offset+len)` range
/// must lie entirely within one segment. A range that straddles two adjacent
/// segments returns [`ExtractTemplateDefError::DataOutOfRange`] even if the
/// bytes would be contiguous in linear memory after instantiation. This is
/// fine for `tari_template_lib`-compiled templates — rustc emits a single
/// rodata segment that holds the entire ABI blob — but it does not generalise
/// to arbitrary WASM toolchains. See the module-level docs for the proper
/// long-term fix (switch to a WASM custom section).
fn read_data_at<'a>(
    segments: &[(u64, Data<'a>)],
    offset: u64,
    len: usize,
) -> Result<&'a [u8], ExtractTemplateDefError> {
    let len_u64 = len as u64;
    let read_end = offset
        .checked_add(len_u64)
        .ok_or(ExtractTemplateDefError::OffsetOverflow)?;
    for (seg_offset, seg_data) in segments {
        let seg_end = seg_offset
            .checked_add(seg_data.data.len() as u64)
            .ok_or(ExtractTemplateDefError::OffsetOverflow)?;
        if offset >= *seg_offset && read_end <= seg_end {
            // Both casts are bounded: `offset - seg_offset <= seg_data.data.len()`
            // (just verified above), and `seg_data.data.len()` is `usize` by
            // construction, so the subtraction-then-cast cannot truncate.
            let local = (offset - seg_offset) as usize;
            return Ok(&seg_data.data[local..local + len]);
        }
    }
    Err(ExtractTemplateDefError::DataOutOfRange { offset, len })
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractTemplateDefError {
    #[error("WASM parse error: {0}")]
    Parse(#[from] BinaryReaderError),
    #[error("Module does not export `{ABI_TEMPLATE_DEF_GLOBAL_NAME}`")]
    AbiExportMissing,
    #[error("`{ABI_TEMPLATE_DEF_GLOBAL_NAME}` resolves to an imported global, not a local one")]
    AbiExportImported,
    #[error("`{ABI_TEMPLATE_DEF_GLOBAL_NAME}` initializer is not an i32.const")]
    AbiGlobalNotConst,
    #[error("ABI length prefix ({length}) exceeds the WASM binary size ({binary_size})")]
    AbiLengthExceedsBinary { length: usize, binary_size: usize },
    #[error("Module imports more globals than fit in u32")]
    TooManyImportedGlobals,
    #[error("Arithmetic overflow computing memory offset")]
    OffsetOverflow,
    #[error("ABI bytes at offset {offset} (len {len}) are not covered by any active data segment")]
    DataOutOfRange { offset: u64, len: usize },
    #[error("Failed to decode TemplateDef: {0}")]
    Decode(#[source] tari_bor::BorError),
}

#[cfg(test)]
mod tests {
    use tari_template_builtin::all_builtin_templates;

    use super::*;

    #[test]
    fn extracts_builtin_template_defs_without_compiling() {
        for template in all_builtin_templates() {
            let def = extract_template_def(template.binary)
                .unwrap_or_else(|e| panic!("extract failed for {}: {}", template.name, e));
            assert_eq!(
                def.template_name(),
                template.name,
                "extracted template_name should match the static builtin name",
            );
        }
    }
}
