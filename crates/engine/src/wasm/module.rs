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

use std::{fmt, fmt::Formatter, sync::Arc};

use tari_engine_types::limits::{self, ModuleShape};
use tari_template_abi::{FunctionDef, FunctionKind, TEMPLATE_DEF_CUSTOM_SECTION, TemplateDef, Type, WASM_PTR_SIZE};
use wasmer::{
    Engine,
    ExternType,
    Function,
    FunctionType,
    Instance,
    Store,
    TypedFunction,
    WasmPtr,
    imports,
    wasmparser::{Parser, Payload},
};

use crate::{
    template::{LoadedTemplate, TemplateLoaderError, TemplateModuleLoader},
    wasm::{
        InitialState,
        WasmExecutionError,
        WasmProcess,
        WasmValidationError,
        engine,
        module_shape::{ModuleStructure, validate_module_structure},
    },
};

pub type MainFunction = TypedFunction<(WasmPtr<u8>, u32), WasmPtr<u8>>;
#[derive(Debug, Clone)]
pub struct WasmModule {
    code: Box<[u8]>,
}

impl WasmModule {
    pub fn from_code(code: impl Into<Box<[u8]>>) -> Self {
        Self { code: code.into() }
    }

    /// Every admission rule a published module must pass that the module bytes alone can answer.
    ///
    /// Split out from [`Self::validate_code`] so a publisher can be charged for the compile between
    /// this and [`Self::compile_prevalidated`]: cranelift is the expensive half, and a module
    /// refused by a rule here never reaches it, so it must not be billed as though it had.
    pub fn prevalidate_code(code: &[u8]) -> Result<ModuleStructure, TemplateLoaderError> {
        // Reject custom sections the engine does not consume. Only the registration path runs this;
        // already-stored templates (and built-ins) load via `load_template_from_code` without it.
        reject_disallowed_custom_sections(code).map_err(WasmExecutionError::from)?;
        let structure = validate_module_structure(code).map_err(WasmExecutionError::from)?;
        Ok(structure)
    }

    /// Compiles a module whose [`Self::prevalidate_code`] pass has already run, so neither of that
    /// pass's two walks over the binary is repeated.
    pub fn compile_prevalidated(code: &[u8], shape: ModuleShape) -> Result<TemplateDef, TemplateLoaderError> {
        // TODO: evaluate if there are acceptable cheaper ways to fully validate
        let loaded = Self::compile(code, shape)?;
        Ok(loaded.into_template_def())
    }

    pub fn validate_code(code: &[u8]) -> Result<TemplateDef, TemplateLoaderError> {
        let structure = Self::prevalidate_code(code)?;
        Self::compile_prevalidated(code, structure.shape)
    }

    pub fn load_template_from_code(code: &[u8]) -> Result<LoadedTemplate, TemplateLoaderError> {
        let structure = validate_module_structure(code).map_err(WasmExecutionError::from)?;
        Self::compile(code, structure.shape)
    }

    fn compile(code: &[u8], shape: ModuleShape) -> Result<LoadedTemplate, TemplateLoaderError> {
        let engine = Self::create_engine();
        let module = wasmer::Module::new(&engine, code)?;
        Self::finalize_loaded_module(engine, module, code.len(), shape)
    }

    /// Load a template from a previously serialized wasmer module (see
    /// [`wasmer::Module::serialize`]). `code_size` is the size of the original
    /// WASM source bytes — preserved from the source compile and used by
    /// downstream caches (e.g. the in-memory moka weigher in
    /// `MemoryCacheTemplateProvider`).
    ///
    /// Takes [`bytes::Bytes`] so callers can pass mmap-backed regions through
    /// without a copy: `wasmer::Module::deserialize_unchecked` accepts `Bytes`
    /// directly, and [`bytes::Bytes::from_owner`] wraps any
    /// `AsRef<[u8]> + Send + 'static` (such as [`memmap2::Mmap`]) without
    /// copying. With `&[u8]`, wasmer's `IntoBytes` impl falls back to
    /// `to_vec()` and we'd pay an extra full-artifact allocation on every
    /// cache hit.
    ///
    /// # Safety
    ///
    /// The serialized bytes MUST have been produced by `wasmer::Module::serialize`
    /// against an engine configured identically to [`Self::create_engine`]. Feeding
    /// arbitrary bytes here is undefined behaviour. Callers are expected to gate
    /// this behind a node-local cache directory whose contents only this process
    /// writes.
    #[cfg(feature = "wasm-cache")]
    pub unsafe fn load_template_from_serialized(
        serialized: bytes::Bytes,
        code_size: usize,
        shape: ModuleShape,
    ) -> Result<LoadedTemplate, TemplateLoaderError> {
        let engine = Self::create_engine();
        // SAFETY: forwarded to caller — see function-level docs.
        let module = unsafe { wasmer::Module::deserialize_unchecked(&engine, serialized) }?;
        Self::finalize_loaded_module(engine, module, code_size, shape)
    }

    /// Validates a compiled module and turns it into a [`LoadedTemplate`].
    ///
    /// Every check the module itself can answer runs before the module is instantiated.
    /// Instantiating creates the guest's linear memory and tables, sized by values the module
    /// declares, so a module reaches it only once the engine has accepted its ABI and its exports.
    fn finalize_loaded_module(
        engine: Engine,
        module: wasmer::Module,
        code_size: usize,
        shape: ModuleShape,
    ) -> Result<LoadedTemplate, TemplateLoaderError> {
        let template = load_template_def_from_custom_section(&module)?;
        let main_fn = format!("{}_main", template.template_name());

        WasmProcess::validate_template_abi_version(&template)?;
        validate_functions(&template)?;
        validate_module_exports(&module, &main_fn)?;

        let mut store = Store::new(engine);
        let imports = imports! {
            "env" => {
                "tari_engine" => Function::new_typed(&mut store, |_op: i32, _arg_ptr: i32, _arg_len: i32| 0i32),
                "on_panic" => Function::new_typed(&mut store, |_msg_ptr: i32, _msg_len: i32, _line: i32, _col: i32| {  }),
            }
        };
        // The memory and table limits live in [`LimitingTunables`], which only sees a module's
        // declared types when the instance's storage is created. Instantiating here is what applies
        // them, so a template that declares more than a limit allows is refused at load rather than
        // at its first call.
        //
        // The instance has run no code, so its state is exactly what every instantiation of the
        // module starts from, and what a reused instance is restored to before each call.
        let instance = Instance::new(&mut store, &module, &imports)?;
        let initial_state = InitialState::capture(&mut store, &instance)?;

        let engine = store.engine().clone();

        Ok(LoadedWasmTemplate::new(template, module, engine, code_size, shape, initial_state).into())
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }

    pub fn into_code(self) -> Box<[u8]> {
        self.code
    }

    fn create_engine() -> Engine {
        engine::create_engine()
    }
}

impl TemplateModuleLoader for WasmModule {
    fn load_template(&self) -> Result<LoadedTemplate, TemplateLoaderError> {
        Self::load_template_from_code(&self.code)
    }
}

#[derive(Clone)]
pub struct LoadedWasmTemplate {
    template_def: Arc<TemplateDef>,
    module: wasmer::Module,
    engine: Engine,
    code_size: usize,
    shape: ModuleShape,
    initial_state: Arc<InitialState>,
}

impl LoadedWasmTemplate {
    pub fn new(
        template_def: TemplateDef,
        module: wasmer::Module,
        engine: Engine,
        code_size: usize,
        shape: ModuleShape,
        initial_state: InitialState,
    ) -> Self {
        Self {
            template_def: Arc::new(template_def),
            module,
            engine,
            code_size,
            shape,
            initial_state: Arc::new(initial_state),
        }
    }

    pub fn wasm_module(&self) -> &wasmer::Module {
        &self.module
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn create_store(&self) -> Store {
        Store::new(self.engine.clone())
    }

    pub fn template_name(&self) -> &str {
        self.template_def.template_name()
    }

    pub fn template_def(&self) -> &TemplateDef {
        &self.template_def
    }

    /// A handle to the definition shared with every other clone of this template, for callers that must hold it past
    /// a move of the template itself.
    pub fn shared_template_def(&self) -> Arc<TemplateDef> {
        self.template_def.clone()
    }

    pub fn into_template_def(self) -> TemplateDef {
        Arc::try_unwrap(self.template_def).unwrap_or_else(|arc| (*arc).clone())
    }

    pub fn find_func_by_name(&self, function_name: &str) -> Option<&FunctionDef> {
        self.template_def.functions().iter().find(|f| f.name == *function_name)
    }

    pub fn code_size(&self) -> usize {
        self.code_size
    }

    pub fn shape(&self) -> ModuleShape {
        self.shape
    }

    /// The state every instance of this template starts a call from.
    pub fn initial_state(&self) -> &InitialState {
        &self.initial_state
    }
}

impl fmt::Debug for LoadedWasmTemplate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedWasmTemplate")
            .field("template_name", &self.template_name())
            .field("code_size", &self.code_size())
            .field("main", &"<main func>")
            .field("module", &self.module)
            .finish()
    }
}

/// Recovers the `TemplateDef` from the `tari_tdef` custom section, which every template must
/// carry. The section is part of the module, so reading it needs no instance and no access to guest
/// memory.
///
/// Wasmer preserves custom sections through compile and `serialize` /
/// `deserialize`, so this works on both freshly compiled modules and modules
/// loaded from the disk cache.
fn load_template_def_from_custom_section(module: &wasmer::Module) -> Result<TemplateDef, WasmExecutionError> {
    let mut sections = module.custom_sections(TEMPLATE_DEF_CUSTOM_SECTION);
    let Some(section) = sections.next() else {
        return Err(WasmExecutionError::AbiTemplateDefSectionMissing);
    };
    // The macro emits exactly one `tari_tdef` section per template. Multiple
    // sections with this name would be ambiguous — refuse to guess which one
    // is canonical.
    if sections.next().is_some() {
        return Err(WasmExecutionError::AbiTemplateDefSectionMalformed {
            reason: format!(
                "module contains more than one `{}` custom section",
                TEMPLATE_DEF_CUSTOM_SECTION
            ),
        });
    }
    if section.len() < WASM_PTR_SIZE {
        return Err(WasmExecutionError::AbiTemplateDefSectionMalformed {
            reason: format!(
                "section is {} bytes; expected at least {} for the length prefix",
                section.len(),
                WASM_PTR_SIZE
            ),
        });
    }
    let prefix: [u8; WASM_PTR_SIZE] = section[..WASM_PTR_SIZE]
        .try_into()
        .expect("section.len() >= WASM_PTR_SIZE checked above");
    let full_len = u32::from_le_bytes(prefix) as usize;
    if full_len != section.len() {
        return Err(WasmExecutionError::AbiTemplateDefSectionMalformed {
            reason: format!(
                "declared length {} is inconsistent with section size {}",
                full_len,
                section.len()
            ),
        });
    }
    let template = tari_bor::decode_exact_with_max_depth::<TemplateDef>(
        &section[WASM_PTR_SIZE..full_len],
        limits::MAX_CBOR_NESTING_DEPTH,
    )
    .map_err(WasmExecutionError::AbiTemplateDefDecodeError)?;
    Ok(template)
}

/// Custom sections the engine consumes and therefore admits into a published
/// template. Everything else (DWARF `.debug_*`, the `name` section,
/// `producers`, …) is semantically inert — cranelift ignores it — but is stored
/// verbatim with the template and replicated across the whole validator
/// committee, so it is rejected at registration.
const ALLOWED_CUSTOM_SECTIONS: &[&str] = &[TEMPLATE_DEF_CUSTOM_SECTION];

/// Reject a published template that carries any custom section other than those
/// in [`ALLOWED_CUSTOM_SECTIONS`].
///
/// The scan defers to the cranelift compile in
/// [`WasmModule::load_template_from_code`] for malformed input: a binary that
/// `wasmparser` cannot parse will also fail wasmer's validation, so stopping at
/// the first parse error lets that path report the canonical `CompileError`
/// rather than a less precise one here.
fn reject_disallowed_custom_sections(code: &[u8]) -> Result<(), WasmValidationError> {
    for payload in Parser::new(0).parse_all(code) {
        // Malformed wasm: stop and let the cranelift compile in
        // `load_template_from_code` report the canonical CompileError.
        let Ok(payload) = payload else { break };
        if let Payload::CustomSection(reader) = payload &&
            !ALLOWED_CUSTOM_SECTIONS.contains(&reader.name())
        {
            return Err(WasmValidationError::DisallowedCustomSection {
                name: reader.name().to_string(),
            });
        }
    }
    Ok(())
}

/// Checks a module's exports against the template ABI, reading them off the module rather than an
/// instance: the memory the engine reads and writes, the three functions it calls and their
/// signatures, and that the module exports no other function.
fn validate_module_exports(module: &wasmer::Module, main_fn: &str) -> Result<(), WasmExecutionError> {
    // `(call_info_ptr: i32, call_info_len: i32) -> i32`, matching [`MainFunction`].
    let expected_main = FunctionType::new([wasmer::Type::I32, wasmer::Type::I32], [wasmer::Type::I32]);
    // `(len: i32) -> i32` and `(ptr: i32)`, matching `WasmAllocFn` and `WasmFreeFn`.
    let expected_alloc = FunctionType::new([wasmer::Type::I32], [wasmer::Type::I32]);
    let expected_free = FunctionType::new([wasmer::Type::I32], []);

    let mut memory_export = false;
    let mut main_signature = None;
    let mut alloc_signature = None;
    let mut free_signature = None;

    for export in module.exports() {
        match export.ty() {
            ExternType::Function(signature) => match export.name() {
                name if name == main_fn => main_signature = Some(signature.clone()),
                "tari_alloc" => alloc_signature = Some(signature.clone()),
                "tari_free" => free_signature = Some(signature.clone()),
                name => {
                    return Err(WasmExecutionError::UnexpectedAbiFunction { name: name.to_string() });
                },
            },
            ExternType::Memory(_) => {
                memory_export |= export.name() == "memory";
            },
            ExternType::Global(_) | ExternType::Table(_) | ExternType::Tag(_) => {},
        }
    }

    if !memory_export {
        return Err(WasmValidationError::MissingExport {
            name: "memory".to_string(),
        }
        .into());
    }

    validate_export_signature(main_fn, main_signature.as_ref(), &expected_main)?;
    validate_export_signature("tari_alloc", alloc_signature.as_ref(), &expected_alloc)?;
    validate_export_signature("tari_free", free_signature.as_ref(), &expected_free)?;

    Ok(())
}

/// The engine calls each ABI function by name and by type, so a module that exports one under
/// another signature — or not at all — is refused at admission rather than on its first call.
fn validate_export_signature(
    name: &str,
    signature: Option<&FunctionType>,
    expected: &FunctionType,
) -> Result<(), WasmValidationError> {
    match signature {
        Some(signature) if signature == expected => Ok(()),
        Some(signature) => Err(WasmValidationError::InvalidExportSignature {
            name: name.to_string(),
            signature: signature.to_string(),
            expected: expected.to_string(),
        }),
        None => Err(WasmValidationError::MissingExport { name: name.to_string() }),
    }
}

fn validate_functions(template_def: &TemplateDef) -> Result<(), WasmExecutionError> {
    match template_def {
        TemplateDef::V2(def) => {
            let function_count = def.functions.len();
            if function_count > limits::WASM_LIMITS.max_functions {
                return Err(WasmValidationError::TooManyFunctions {
                    max_functions: limits::WASM_LIMITS.max_functions,
                }
                .into());
            }
            for func in &def.functions {
                if func.name.len() > limits::WASM_LIMITS.max_function_name_length {
                    return Err(WasmValidationError::FunctionNameTooLong {
                        name: func.name.clone(),
                        max_length: limits::WASM_LIMITS.max_function_name_length,
                    }
                    .into());
                }

                if func.num_call_args() > limits::WASM_LIMITS.max_function_arguments {
                    return Err(WasmValidationError::FunctionTooManyArguments {
                        name: func.name.clone(),
                        max_args: limits::WASM_LIMITS.max_function_arguments,
                        num_args: func.num_call_args(),
                    }
                    .into());
                }
                for arg in &func.arguments {
                    if arg.name.len() > limits::WASM_LIMITS.max_function_name_length {
                        return Err(WasmValidationError::FunctionNameTooLong {
                            name: arg.name.clone(),
                            max_length: limits::WASM_LIMITS.max_function_name_length,
                        }
                        .into());
                    }
                    validate_abi_type(&func.name, &arg.arg_type)?;
                }
                validate_abi_type(&func.name, &func.output)?;
                if let FunctionKind::Migration { from } = &func.kind {
                    validate_abi_type(&func.name, from)?;
                    // Note that we are checking the TemplateDef, not the actual return type of the function in Wasm.
                    match &func.output {
                        Type::Other { name } => {
                            if name != "Self" &&
                                name != template_def.template_name() &&
                                name != "Component<Self>" &&
                                *name != format!("Component<{}>", template_def.template_name())
                            {
                                return Err(WasmValidationError::InvalidMigrationReturnType {
                                    function_name: func.name.clone(),
                                    return_type: func.output.clone(),
                                }
                                .into());
                            }
                        },
                        _ => {
                            return Err(WasmValidationError::InvalidMigrationReturnType {
                                function_name: func.name.clone(),
                                return_type: func.output.clone(),
                            }
                            .into());
                        },
                    }
                }
            }
        },
    }
    Ok(())
}

/// Holds an ABI type `function` declares, and every type nested in it, to the depth, size, tuple width and name
/// length limits.
fn validate_abi_type(function: &str, ty: &Type) -> Result<(), WasmValidationError> {
    let limits = &limits::WASM_LIMITS;
    let mut num_types = 0usize;
    let mut pending = vec![(ty, 1usize)];
    while let Some((ty, depth)) = pending.pop() {
        num_types += 1;
        if num_types > limits.max_abi_type_nodes {
            return Err(WasmValidationError::AbiTypeTooLarge {
                function: function.to_string(),
                max_types: limits.max_abi_type_nodes,
            });
        }
        if depth > limits.max_abi_type_depth {
            return Err(WasmValidationError::AbiTypeTooDeep {
                function: function.to_string(),
                max_depth: limits.max_abi_type_depth,
            });
        }
        match ty {
            Type::Vec(inner) | Type::Option(inner) => pending.push((inner, depth + 1)),
            Type::Tuple(types) => {
                if types.len() > limits.max_function_arguments {
                    return Err(WasmValidationError::FunctionTooManyTupleReturn {
                        name: function.to_string(),
                        max_tuple_size: limits.max_function_arguments,
                        tuple_size: types.len(),
                    });
                }
                pending.extend(types.iter().map(|t| (t, depth + 1)));
            },
            Type::Other { name } if name.len() > limits.max_function_name_length => {
                return Err(WasmValidationError::AbiTypeNameTooLong {
                    function: function.to_string(),
                    length: name.len(),
                    max_length: limits.max_function_name_length,
                });
            },
            _ => {},
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Build a header-only WASM module carrying the given named custom sections,
    /// in order. `wasmparser` accepts magic+version plus custom sections, which
    /// is all `reject_disallowed_custom_sections` inspects.
    fn wasm_with_custom_sections(sections: &[(&str, &[u8])]) -> Vec<u8> {
        let mut wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        for (name, payload) in sections {
            let mut body = Vec::new();
            write_uleb128(&mut body, name.len() as u32);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(payload);
            wasm.push(0); // custom section id
            write_uleb128(&mut wasm, body.len() as u32);
            wasm.extend_from_slice(&body);
        }
        wasm
    }

    #[test]
    fn accepts_module_without_custom_sections() {
        let wasm = wasm_with_custom_sections(&[]);
        reject_disallowed_custom_sections(&wasm).expect("no custom sections is allowed");
    }

    #[test]
    fn accepts_only_template_def_section() {
        let wasm = wasm_with_custom_sections(&[(TEMPLATE_DEF_CUSTOM_SECTION, &[1, 2, 3, 4])]);
        reject_disallowed_custom_sections(&wasm).expect("tari_tdef is allowed");
    }

    #[test]
    fn rejects_name_section() {
        let wasm = wasm_with_custom_sections(&[("name", &[0u8; 64])]);
        match reject_disallowed_custom_sections(&wasm) {
            Err(WasmValidationError::DisallowedCustomSection { name }) => assert_eq!(name, "name"),
            other => panic!("expected DisallowedCustomSection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_dwarf_debug_section() {
        let wasm = wasm_with_custom_sections(&[(".debug_info", &[0u8; 1024])]);
        match reject_disallowed_custom_sections(&wasm) {
            Err(WasmValidationError::DisallowedCustomSection { name }) => assert_eq!(name, ".debug_info"),
            other => panic!("expected DisallowedCustomSection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_disallowed_section_alongside_template_def() {
        let wasm =
            wasm_with_custom_sections(&[(TEMPLATE_DEF_CUSTOM_SECTION, &[1, 2, 3, 4]), ("producers", &[0u8; 16])]);
        match reject_disallowed_custom_sections(&wasm) {
            Err(WasmValidationError::DisallowedCustomSection { name }) => assert_eq!(name, "producers"),
            other => panic!("expected DisallowedCustomSection, got {other:?}"),
        }
    }

    fn template_def_with_func(
        kind: FunctionKind,
        arguments: Vec<tari_template_abi::ArgDef>,
        output: Type,
    ) -> TemplateDef {
        TemplateDef::V2(tari_template_abi::TemplateDefV2 {
            template_name: "Test".to_string(),
            abi_version: 0,
            functions: vec![FunctionDef {
                name: "f".to_string(),
                kind,
                arguments,
                output,
            }],
        })
    }

    #[test]
    fn rejects_oversized_tuple_in_function_output() {
        let max = limits::WASM_LIMITS.max_function_arguments;
        let def = template_def_with_func(
            FunctionKind::Function,
            vec![],
            Type::Tuple(vec![Type::U8; max + 1]),
        );
        match validate_functions(&def) {
            Err(WasmExecutionError::WasmValidationError(WasmValidationError::FunctionTooManyTupleReturn {
                name,
                max_tuple_size,
                tuple_size,
            })) => {
                assert_eq!(name, "f");
                assert_eq!(max_tuple_size, max);
                assert_eq!(tuple_size, max + 1);
            },
            other => panic!("expected FunctionTooManyTupleReturn, got {other:?}"),
        }
    }

    #[test]
    fn rejects_oversized_type_name_in_function_output_and_nested_types() {
        let max_len = limits::WASM_LIMITS.max_function_name_length;
        let long_name = "T".repeat(max_len + 1);
        let long_other = Type::Other {
            name: long_name.clone(),
        };

        for bad_type in [
            long_other.clone(),
            Type::Vec(Box::new(long_other.clone())),
            Type::Option(Box::new(long_other.clone())),
            Type::Result {
                ok: Box::new(Type::Unit),
                err: Box::new(long_other.clone()),
            },
            Type::Tuple(vec![Type::U8, long_other]),
        ] {
            let def = template_def_with_func(FunctionKind::Function, vec![], bad_type.clone());
            match validate_functions(&def) {
                Err(WasmExecutionError::WasmValidationError(WasmValidationError::FunctionNameTooLong {
                    name,
                    max_length,
                })) => {
                    assert_eq!(name, long_name);
                    assert_eq!(max_length, max_len);
                },
                other => panic!("expected FunctionNameTooLong for output {bad_type:?}, got {other:?}"),
            }

            let def = template_def_with_func(
                FunctionKind::Function,
                vec![tari_template_abi::ArgDef {
                    name: "x".to_string(),
                    arg_type: bad_type.clone(),
                }],
                Type::Unit,
            );
            match validate_functions(&def) {
                Err(WasmExecutionError::WasmValidationError(WasmValidationError::FunctionNameTooLong {
                    name,
                    max_length,
                })) => {
                    assert_eq!(name, long_name);
                    assert_eq!(max_length, max_len);
                },
                other => panic!("expected FunctionNameTooLong for arg {bad_type:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_nested_oversized_tuple_in_arg_and_output() {
        let max = limits::WASM_LIMITS.max_function_arguments;
        let nested = Type::Option(Box::new(Type::Vec(Box::new(Type::Tuple(vec![Type::U8; max + 1])))));
        let def = template_def_with_func(
            FunctionKind::Function,
            vec![tari_template_abi::ArgDef {
                name: "x".to_string(),
                arg_type: nested,
            }],
            Type::Unit,
        );
        match validate_functions(&def) {
            Err(WasmExecutionError::WasmValidationError(WasmValidationError::FunctionTooManyTupleReturn {
                name,
                max_tuple_size,
                tuple_size,
            })) => {
                assert_eq!(name, "f");
                assert_eq!(max_tuple_size, max);
                assert_eq!(tuple_size, max + 1);
            },
            other => panic!("expected FunctionTooManyTupleReturn, got {other:?}"),
        }
    }
}
