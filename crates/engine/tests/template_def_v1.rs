//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A template binary carries the definition layout of the macro that built it, so every template published before
//! the V2 layout embeds a V1 definition: each method's receiver is a leading `self` argument and `is_mut` sets its
//! component lock. These tests run such a template end to end.

use tari_engine::wasm::WasmModule;
use tari_engine_types::hashing::hash_template_code;
use tari_ootle_transaction::args;
use tari_template_abi::{
    ArgDef,
    FunctionDefV1,
    Receiver,
    TEMPLATE_DEF_CUSTOM_SECTION,
    TemplateDef,
    TemplateDefV1,
    Type,
    VersionedTemplateDef,
    WASM_PTR_SIZE,
};
use tari_template_lib::types::{ComponentAddress, TemplateAddress};
use tari_template_test_tooling::{Package, TemplateTest, compile::compile_template};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");
const WASM_HEADER_LEN: usize = 8;

/// The V1 layout of `def`, as the macro emitted it before V2.
fn to_v1(def: &TemplateDef) -> VersionedTemplateDef {
    let functions = def
        .functions()
        .iter()
        .map(|f| {
            let receiver = f.receiver.map(|receiver| ArgDef {
                name: "self".to_string(),
                arg_type: Type::Other {
                    name: match receiver {
                        Receiver::Ref => "&self",
                        Receiver::RefMut => "&mut self",
                    }
                    .to_string(),
                },
            });
            FunctionDefV1 {
                name: f.name.clone(),
                arguments: receiver.into_iter().chain(f.arguments.iter().cloned()).collect(),
                output: f.output.clone(),
                is_mut: f.is_mut(),
                is_migration: f.is_migration,
            }
        })
        .collect();
    VersionedTemplateDef::V1(TemplateDefV1 {
        template_name: def.template_name().to_string(),
        abi_version: def.abi_version(),
        functions,
    })
}

fn write_uleb128(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_uleb128(code: &[u8], pos: &mut usize) -> usize {
    let mut value = 0;
    let mut shift = 0;
    loop {
        let byte = code[*pos];
        *pos += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
        shift += 7;
    }
}

/// `code` with its template definition section replaced by one holding `def`.
fn with_template_def(code: &[u8], def: &VersionedTemplateDef) -> Vec<u8> {
    let encoded = tari_bor::encode(def).unwrap();
    let mut blob = Vec::with_capacity(WASM_PTR_SIZE + encoded.len());
    blob.extend_from_slice(&u32::try_from(WASM_PTR_SIZE + encoded.len()).unwrap().to_le_bytes());
    blob.extend_from_slice(&encoded);

    let name = TEMPLATE_DEF_CUSTOM_SECTION.as_bytes();
    let mut body = Vec::new();
    write_uleb128(&mut body, name.len());
    body.extend_from_slice(name);
    body.extend_from_slice(&blob);

    let mut out = code[..WASM_HEADER_LEN].to_vec();
    let mut replaced = false;
    let mut pos = WASM_HEADER_LEN;
    while pos < code.len() {
        let start = pos;
        let id = code[pos];
        pos += 1;
        let size = read_uleb128(code, &mut pos);
        let payload = pos;
        pos += size;
        if id == 0 {
            let mut name_pos = payload;
            let name_len = read_uleb128(code, &mut name_pos);
            if &code[name_pos..name_pos + name_len] == name {
                out.push(0);
                write_uleb128(&mut out, body.len());
                out.extend_from_slice(&body);
                replaced = true;
                continue;
            }
        }
        out.extend_from_slice(&code[start..pos]);
    }
    assert!(replaced, "the template has no {TEMPLATE_DEF_CUSTOM_SECTION} section");
    out
}

/// The `state` test template, rebuilt with a V1 definition.
fn v1_state_template() -> (Vec<u8>, TemplateDef) {
    let module = compile_template(format!("{CRATE_PATH}/tests/templates/state"), &[]).unwrap();
    let current = WasmModule::validate_code(module.code()).unwrap();
    (with_template_def(module.code(), &to_v1(&current)), current)
}

fn load(code: Vec<u8>) -> (TemplateTest, TemplateAddress) {
    let address: TemplateAddress = hash_template_code(&code);
    let mut builder = Package::builder();
    builder.add_all_builtin_templates();
    builder
        .add_template_from_code(address, code)
        .expect("template was rejected by the loader");
    let mut test = TemplateTest::from_package(builder.build());
    test.bootstrap_state();
    (test, address)
}

#[test]
fn a_v1_definition_loads_as_the_definition_the_macro_emits() {
    let (code, current) = v1_state_template();

    let loaded = WasmModule::validate_code(&code).unwrap();

    for expected in current.functions() {
        let function = loaded.get_function(&expected.name).unwrap();
        assert_eq!(function.receiver, expected.receiver, "{}", expected.name);
        assert_eq!(function.arguments.len(), expected.arguments.len(), "{}", expected.name);
    }
    assert_eq!(loaded.get_function("set").unwrap().receiver, Some(Receiver::RefMut));
    assert_eq!(loaded.get_function("get").unwrap().receiver, Some(Receiver::Ref));
    assert_eq!(loaded.get_function("new").unwrap().receiver, None);
}

/// `set` writes the component, which needs the write lock `is_mut` asks for, and takes one argument after the
/// component address. `get` reads it back under a read lock.
#[test]
fn a_v1_template_runs_its_methods() {
    let (code, _) = v1_state_template();
    let (mut test, address) = load(code);

    test.execute_expect_success(
        test.transaction()
            .call_function(address, "new", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    let component: ComponentAddress = test
        .read_only_state_store()
        .get_first_component_of(address)
        .unwrap()
        .unwrap();

    test.call_method::<()>(component, "set", args![7u32], vec![]);
    let value: u32 = test.call_method(component, "get", args![], vec![]);
    assert_eq!(value, 7);
}
