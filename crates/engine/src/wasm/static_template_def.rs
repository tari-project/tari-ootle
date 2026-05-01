//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Static extraction of a template's `TemplateDef` directly from WASM bytes,
//! without invoking cranelift.
//!
//! Templates compiled from the `tari_template_lib` macros embed their ABI
//! definition as a tari-bor-encoded blob in the module's data section,
//! addressed by an exported global named [`ABI_TEMPLATE_DEF_GLOBAL_NAME`].
//! `WasmEnv::load_template_def` recovers it via wasmer instantiation and
//! linear-memory access; `WasmModule::extract_template_def` recovers the same
//! bytes by parsing the WASM binary statically (no compile, no JIT, no
//! linear memory).
//!
//! This is intended for callers that only need the type/function metadata
//! (e.g. the wallet daemon's template monitor) and want to avoid pulling in
//! cranelift's compile cost. It does **not** validate the WASM module —
//! anyone using a template for execution should still go through
//! `WasmModule::load_template_from_code`.

use tari_template_abi::{ABI_TEMPLATE_DEF_GLOBAL_NAME, TemplateDef, WASM_PTR_SIZE};
use wasmer::wasmparser::{BinaryReaderError, Data, DataKind, ExternalKind, Operator, Parser, Payload, TypeRef};

/// Statically extract the embedded `TemplateDef` from a template's WASM bytes.
///
/// Cheap relative to a full compile: parses only the export, global, import
/// and data sections needed to locate and read the ABI blob. No cranelift, no
/// instantiation, no linear-memory allocation.
pub fn extract_template_def(code: &[u8]) -> Result<TemplateDef, ExtractTemplateDefError> {
    let mut imported_global_count: u32 = 0;
    let mut globals_init: Vec<Option<i32>> = Vec::new();
    let mut export_global_idx: Option<u32> = None;
    let mut data_segments= Vec::new();

    for payload in Parser::new(0).parse_all(code) {
        match payload? {
            Payload::ImportSection(reader) => {
                for imports in reader {
                    for entry in imports? {
                        let (_, import) = entry?;
                        if matches!(import.ty, TypeRef::Global(_)) {
                            imported_global_count += 1;
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
                    if let DataKind::Active { memory_index: 0, offset_expr } = segment.kind {
                        let mut ops = offset_expr.get_operators_reader();
                        if let Ok(Operator::I32Const { value }) = ops.read() {
                            data_segments.push((value as u32 as u64, segment));
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
    let abi_ptr = abi_ptr as u32 as u64;

    // [u32 LE: length] || [length bytes: tari-bor-encoded TemplateDef]
    let len_bytes = read_data_at(&data_segments, abi_ptr, WASM_PTR_SIZE)?;
    let length = u32::from_le_bytes(len_bytes.try_into().expect("WASM_PTR_SIZE == 4")) as usize;
    let body = read_data_at(&data_segments, abi_ptr + WASM_PTR_SIZE as u64, length)?;

    tari_bor::decode(&body).map_err(ExtractTemplateDefError::Decode)
}

fn read_data_at<'a>(
    segments: &'a [(u64, Data)],
    offset: u64,
    len: usize,
) -> Result<&'a [u8], ExtractTemplateDefError> {
    let len_u64 = len as u64;
    for (seg_offset, seg_data) in segments {
        let seg_end = seg_offset + seg_data.data.len() as u64;
        if offset >= *seg_offset && offset + len_u64 <= seg_end {
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
