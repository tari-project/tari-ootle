//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Static extraction of a template's `TemplateDef` directly from WASM bytes,
//! without invoking cranelift.
//!
//! Templates compiled from the `tari_template_lib` macros embed their ABI
//! definition in two redundant places:
//!
//! 1. **Custom WASM section** named [`TEMPLATE_DEF_CUSTOM_SECTION`] (`tari_tdef`). The bor-encoded `TemplateDef` is
//!    written directly into the section as `[u32 LE: full_len] || [bor bytes]`. This is the canonical extraction path:
//!    independent of linear-memory layout, requires no global / data-segment walk, and works for any WASM toolchain
//!    that can emit a custom section.
//! 2. **Exported i32 global** named [`ABI_TEMPLATE_DEF_GLOBAL_NAME`] (`_ABI_TEMPLATE_DEF`) pointing into a rodata data
//!    segment. Legacy embedding, kept by the macro for backward compatibility with engines that don't read the custom
//!    section.
//!
//! `extract_template_def` here prefers the custom section. If absent (older
//! template binaries that pre-date the custom-section embedding), it falls
//! back to walking the data segments via the global pointer. That fallback
//! path is the more fragile one — see "Fallback toolchain assumptions" below.
//!
//! Intended exclusively for callers that only need the type/function metadata
//! (today: only the wallet daemon's template monitor) and want to avoid the
//! cranelift compile cost. It does **not** validate the WASM module — anyone
//! using a template for execution should go through
//! `WasmModule::load_template_from_code`.
//!
//! # Fallback toolchain assumptions
//!
//! The data-segment fallback is what `rustc`'s `wasm32-unknown-unknown` LLVM
//! backend produces in practice for templates built from `tari_template_lib`'s
//! `#[template]` macro: a single active data segment in memory 0 covering the
//! static rodata, with the `_ABI_TEMPLATE_DEF` global initialised to an
//! `i32.const` pointer into that segment, and the `[u32 LE: length] || [bor
//! bytes]` blob laid out contiguously at that pointer.
//!
//! Specifically, [`read_data_at`] requires the `[length, payload]` blob to lie
//! **entirely within a single active data segment**. Other WASM toolchains
//! (AssemblyScript, emscripten, custom code generators) may split static data
//! across multiple segments or place rodata at arbitrary offsets, in which
//! case the fallback fails with [`ExtractTemplateDefError::DataOutOfRange`].
//! New templates avoid this entirely by using the custom-section path; the
//! fallback only ever runs for legacy binaries built before the macro emitted
//! the custom section.

use tari_template_abi::{ABI_TEMPLATE_DEF_GLOBAL_NAME, TEMPLATE_DEF_CUSTOM_SECTION, TemplateDef, WASM_PTR_SIZE};
use wasmer::wasmparser::{BinaryReaderError, Data, DataKind, ExternalKind, Operator, Parser, Payload, TypeRef};

/// Statically extract the embedded `TemplateDef` from a template's WASM bytes.
///
/// Cheap relative to a full compile: a single linear pass over the WASM
/// payload collects the custom section (and, if needed, the global / data
/// section state for the legacy fallback). No cranelift, no instantiation, no
/// linear-memory allocation.
///
/// All arithmetic on offsets / lengths is checked. Adversarial inputs are
/// rejected with an explicit error rather than panicking or wrapping.
pub fn extract_template_def(code: &[u8]) -> Result<TemplateDef, ExtractTemplateDefError> {
    let mut imported_global_count: u32 = 0;
    let mut globals_init: Vec<Option<i32>> = Vec::new();
    let mut export_global_idx: Option<u32> = None;
    let mut data_segments = Vec::new();

    // TODO: deprecate the use of global sections once all templates are using >=0.26
    for payload in Parser::new(0).parse_all(code) {
        match payload? {
            Payload::CustomSection(reader) if reader.name() == TEMPLATE_DEF_CUSTOM_SECTION => {
                return decode_template_def_from_blob(reader.data());
            },
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

    // Fallback: legacy rodata + global pointer embedding.
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

/// Decode a `[u32 LE: full_len] || [bor bytes]` blob (the format produced by
/// `TemplateDef::encode_for_wasm_embedding`). `full_len` is the total length
/// including the 4-byte prefix itself; the bor payload occupies
/// `blob[4..full_len]`.
fn decode_template_def_from_blob(blob: &[u8]) -> Result<TemplateDef, ExtractTemplateDefError> {
    if blob.len() < WASM_PTR_SIZE {
        return Err(ExtractTemplateDefError::SectionTooShort { len: blob.len() });
    }
    let prefix: [u8; WASM_PTR_SIZE] = blob[..WASM_PTR_SIZE]
        .try_into()
        .expect("blob.len() >= WASM_PTR_SIZE checked above");
    let full_len = u32::from_le_bytes(prefix) as usize;
    if full_len < WASM_PTR_SIZE || full_len > blob.len() {
        return Err(ExtractTemplateDefError::SectionLengthMismatch {
            declared: full_len,
            actual: blob.len(),
        });
    }
    tari_bor::decode(&blob[WASM_PTR_SIZE..full_len]).map_err(ExtractTemplateDefError::Decode)
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
/// to arbitrary WASM toolchains. New templates avoid this fallback entirely
/// by using the `tari_tdef` custom section; see the module-level docs.
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
    #[error(
        "Module does not export `{ABI_TEMPLATE_DEF_GLOBAL_NAME}` and has no `{TEMPLATE_DEF_CUSTOM_SECTION}` custom \
         section"
    )]
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
    #[error("`{TEMPLATE_DEF_CUSTOM_SECTION}` custom section is too short ({len} bytes) to hold a length prefix")]
    SectionTooShort { len: usize },
    #[error("`{TEMPLATE_DEF_CUSTOM_SECTION}` declares length {declared} but the section is {actual} bytes")]
    SectionLengthMismatch { declared: usize, actual: usize },
    #[error("Failed to decode TemplateDef: {0}")]
    Decode(#[source] tari_bor::BorError),
}

#[cfg(test)]
mod tests {
    use tari_template_abi::{TemplateDef, TemplateDefV1, version};
    use tari_template_builtin::all_builtin_templates;

    use super::*;

    /// Encode an unsigned LEB128 integer into `out`. Just enough for the
    /// ranges we use in tests (section sizes, name lengths up to 64 KiB).
    fn write_uleb128(out: &mut Vec<u8>, mut value: u32) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    /// Build a minimal valid WASM binary that contains a single custom section
    /// with the given name and payload. No type/import/export/etc. sections —
    /// the extractor only needs the custom section, and `wasmparser` accepts
    /// modules that have just the magic+version header plus custom sections.
    fn make_wasm_with_custom_section(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut wasm = Vec::with_capacity(8 + 16 + name.len() + payload.len());
        // Magic + version
        wasm.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
        // Custom section: section id 0, size LEB, name length LEB, name bytes, data
        let mut section_body = Vec::with_capacity(name.len() + payload.len() + 8);
        write_uleb128(&mut section_body, name.len() as u32);
        section_body.extend_from_slice(name.as_bytes());
        section_body.extend_from_slice(payload);
        wasm.push(0); // section id 0 = custom
        write_uleb128(&mut wasm, section_body.len() as u32);
        wasm.extend_from_slice(&section_body);
        wasm
    }

    fn synthetic_template_def() -> TemplateDef {
        TemplateDef::V1(TemplateDefV1 {
            template_name: "Synthetic".to_string(),
            abi_version: version::LATEST_TEMPLATE_VERSION,
            functions: Vec::new(),
        })
    }

    #[test]
    fn extracts_legacy_builtin_template_defs() {
        // The committed builtin .wasm files predate the custom-section
        // embedding and exercise the legacy global+rodata fallback path.
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

    #[test]
    fn extracts_from_custom_section() {
        let blob = synthetic_template_def().encode_for_wasm_embedding().expect("encode");
        let wasm = make_wasm_with_custom_section(TEMPLATE_DEF_CUSTOM_SECTION, &blob);
        let def = extract_template_def(&wasm).expect("extract from custom section");
        assert_eq!(def.template_name(), "Synthetic");
    }

    #[test]
    fn rejects_malformed_section_length() {
        // Length prefix declares more bytes than the section actually holds.
        let mut blob = vec![0u8; 16];
        blob[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        let wasm = make_wasm_with_custom_section(TEMPLATE_DEF_CUSTOM_SECTION, &blob);
        match extract_template_def(&wasm) {
            Err(ExtractTemplateDefError::SectionLengthMismatch { .. }) => {},
            other => panic!("expected SectionLengthMismatch, got {:?}", other),
        }
    }
}
