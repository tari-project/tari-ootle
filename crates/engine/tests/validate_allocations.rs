//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The memory [`WasmModule::validate`] allocates for a module. Validation runs before a publish pays
//! for its compile, so what it allocates must stay in proportion to the binary, whatever the module
//! declares.
//!
//! The allocator here counts every allocation in this test binary, so this file holds one test.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use tari_engine::wasm::{WasmExecutionError, WasmModule};
use tari_engine_types::limits;
use tari_template_abi::{FunctionDef, FunctionKind, TemplateDef, TemplateDefV2, Type};

struct CountingAllocator;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: forwarded to the system allocator with the caller's layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// One function with the largest signature wasmparser reads, exported under one name many times.
/// Each export is four bytes of binary and names a 2,000-value signature. Functions are referred to
/// by index, because a named one gives the module a `name` section, which validation refuses first.
#[test]
fn validation_allocates_in_proportion_to_the_binary_however_many_exports_share_a_signature() {
    const EXPORTS: usize = 240_000;
    let def_bytes = TemplateDef::V2(TemplateDefV2 {
        template_name: "Buggy".to_string(),
        abi_version: 0,
        functions: vec![FunctionDef {
            name: "transfer".to_string(),
            kind: FunctionKind::Function,
            arguments: vec![],
            output: Type::Unit,
        }],
    })
    .encode_for_wasm_embedding()
    .unwrap();
    let escaped: String = def_bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    let params = " i64".repeat(1000);
    let exports = r#"(export "x" (func 3))"#.repeat(EXPORTS);
    let code = wat::parse_str(format!(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "tari_alloc") (param i32) (result i32) (i32.const 1024))
          (func (export "tari_free") (param i32))
          (func (export "Buggy_main") (param i32 i32) (result i32) (i32.const 0))
          (func (param{params}) (result{params}) unreachable)
          {exports}
          (@custom "tari_tdef" "{escaped}")
        )
        "#
    ))
    .unwrap();
    assert!(code.len() <= limits::ENGINE_LIMITS.max_template_binary_size_bytes);

    let before = ALLOCATED.load(Ordering::Relaxed);
    let err = WasmModule::validate(&code).expect_err("an unexpected export was admitted");
    let allocated = ALLOCATED.load(Ordering::Relaxed) - before;

    assert!(
        err.to_string()
            .contains(&WasmExecutionError::UnexpectedAbiFunction { name: "x".to_string() }.to_string()),
        "{err}"
    );
    assert!(
        allocated <= code.len(),
        "validating a {}-byte binary allocated {allocated} bytes",
        code.len()
    );
}
