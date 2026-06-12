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

use tari_engine_types::limits;
use tari_template_abi::{
    ABI_TEMPLATE_DEF_GLOBAL_NAME,
    FunctionDef,
    TEMPLATE_DEF_CUSTOM_SECTION,
    TemplateDef,
    Type,
    WASM_PTR_SIZE,
};
use wasmer::{
    AsStoreMut,
    Engine,
    ExportError,
    Function,
    Instance,
    Pages,
    Store,
    TypedFunction,
    WasmPtr,
    imports,
    sys::{BaseTunables, CompilerConfig, Cranelift, CraneliftOptLevel, EngineBuilder, Target},
};

use crate::{
    template::{LoadedTemplate, TemplateLoaderError, TemplateModuleLoader},
    wasm::{
        WasmExecutionError,
        WasmProcess,
        WasmValidationError,
        environment::WasmEnv,
        limiting_tunable::LimitingTunables,
        metering,
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

    pub fn validate_code(code: &[u8]) -> Result<TemplateDef, TemplateLoaderError> {
        // TODO: evaluate if there are acceptable cheaper ways to fully validate
        let loaded = Self::load_template_from_code(code)?;
        Ok(loaded.into_template_def())
    }

    pub fn load_template_from_code(code: &[u8]) -> Result<LoadedTemplate, TemplateLoaderError> {
        let engine = Self::create_engine();
        let module = wasmer::Module::new(&engine, code)?;
        Self::finalize_loaded_module(engine, module, code.len())
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
    ) -> Result<LoadedTemplate, TemplateLoaderError> {
        let engine = Self::create_engine();
        // SAFETY: forwarded to caller — see function-level docs.
        let module = unsafe { wasmer::Module::deserialize_unchecked(&engine, serialized) }?;
        Self::finalize_loaded_module(engine, module, code_size)
    }

    fn finalize_loaded_module(
        engine: Engine,
        module: wasmer::Module,
        code_size: usize,
    ) -> Result<LoadedTemplate, TemplateLoaderError> {
        let mut store = Store::new(engine);

        let imports = imports! {
            "env" => {
                "tari_engine" => Function::new_typed(&mut store, |_op: i32, _arg_ptr: i32, _arg_len: i32| 0i32),
                "tari_debug" => Function::new_typed(&mut store, |_arg_ptr: i32, _arg_len: i32| {  }),
                "on_panic" => Function::new_typed(&mut store, |_msg_ptr: i32, _msg_len: i32, _line: i32, _col: i32| {  }),
            }
        };
        let instance = Instance::new(&mut store, &module, &imports)?;
        let mut env = WasmEnv::new(());
        let memory = instance.exports.get_memory("memory")?.clone();
        env.set_memory(memory);

        // Prefer the `tari_tdef` custom section. New templates produced by the
        // current `#[template]` macro embed the bor-encoded `TemplateDef`
        // there. If the section is absent we treat the binary as legacy and
        // fall back to reading the blob out of linear memory via the
        // `_ABI_TEMPLATE_DEF` exported global.
        let template = match load_template_def_from_custom_section(&module)? {
            Some(def) => def,
            None => env.load_template_def(&mut store, &instance)?,
        };
        let main_fn = format!("{}_main", template.template_name());

        WasmProcess::validate_template_abi_version(&template)?;
        validate_instance(&mut store, &instance, &main_fn)?;
        validate_functions(&template)?;

        let engine = store.engine().clone();

        Ok(LoadedWasmTemplate::new(template, module, engine, code_size).into())
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }

    pub fn into_code(self) -> Box<[u8]> {
        self.code
    }

    fn create_engine() -> Engine {
        const MEMORY_PAGE_LIMIT: Pages = Pages(limits::WASM_LIMITS.max_memory_pages as u32);
        let base = BaseTunables::for_target(&Target::default());
        let tunables = LimitingTunables::new(base, MEMORY_PAGE_LIMIT);
        let mut compiler = Cranelift::new();
        compiler
            .opt_level(CraneliftOptLevel::SpeedAndSize)
            .canonicalize_nans(true);
        // Per-call metering ceiling. `WasmProcess::invoke` lowers each call's allowance further to
        // whatever remains of the per-transaction budget (`MAX_WASM_POINTS_PER_TRANSACTION`).
        compiler.push_middleware(Arc::new(metering::middleware(limits::MAX_WASM_POINTS_PER_CALL)));

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
}

impl LoadedWasmTemplate {
    pub fn new(template_def: TemplateDef, module: wasmer::Module, engine: Engine, code_size: usize) -> Self {
        Self {
            template_def: Arc::new(template_def),
            module,
            engine,
            code_size,
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

    pub fn into_template_def(self) -> TemplateDef {
        Arc::try_unwrap(self.template_def).unwrap_or_else(|arc| (*arc).clone())
    }

    pub fn find_func_by_name(&self, function_name: &str) -> Option<&FunctionDef> {
        self.template_def.functions().iter().find(|f| f.name == *function_name)
    }

    pub fn code_size(&self) -> usize {
        self.code_size
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

/// Try to recover the `TemplateDef` directly from the `tari_tdef` custom
/// section. Returns `Ok(None)` when the section is absent (legacy templates
/// that only embed the ABI via the `_ABI_TEMPLATE_DEF` global+rodata
/// pattern); the caller falls back to reading the blob out of linear memory
/// in that case.
///
/// Wasmer preserves custom sections through compile and `serialize` /
/// `deserialize`, so this works on both freshly compiled modules and modules
/// loaded from the disk cache.
fn load_template_def_from_custom_section(module: &wasmer::Module) -> Result<Option<TemplateDef>, WasmExecutionError> {
    let mut sections = module.custom_sections(TEMPLATE_DEF_CUSTOM_SECTION);
    let Some(section) = sections.next() else {
        return Ok(None);
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
    if full_len < WASM_PTR_SIZE || full_len > section.len() {
        return Err(WasmExecutionError::AbiTemplateDefSectionMalformed {
            reason: format!(
                "declared length {} is inconsistent with section size {}",
                full_len,
                section.len()
            ),
        });
    }
    let template = tari_bor::decode::<TemplateDef>(&section[WASM_PTR_SIZE..full_len])
        .map_err(WasmExecutionError::AbiTemplateDefDecodeError)?;
    Ok(Some(template))
}

fn validate_instance<S: AsStoreMut>(
    store: &mut S,
    instance: &Instance,
    main_fn: &str,
) -> Result<(), WasmExecutionError> {
    fn is_func_permitted(name: &str, main_fn: &str) -> bool {
        name == main_fn || name == "tari_alloc" || name == "tari_free"
    }

    instance.exports.get_memory("memory")?;

    // Enforce that only permitted functions are allowed
    let unexpected_abi_func = instance
        .exports
        .iter()
        .functions()
        .find(|(name, _)| !is_func_permitted(name, main_fn));

    if let Some((name, _)) = unexpected_abi_func {
        return Err(WasmExecutionError::UnexpectedAbiFunction { name: name.to_string() });
    }

    // The `_ABI_TEMPLATE_DEF` global is present in legacy templates (where
    // the ABI lives in linear memory) and absent in templates produced by
    // the current macro (which puts the ABI in the `tari_tdef` custom
    // section). When present, sanity-check that it's an i32; when missing,
    // we've already validated the ABI via the custom section.
    if let Ok(global) = instance.exports.get_global(ABI_TEMPLATE_DEF_GLOBAL_NAME) {
        global
            .get(store)
            .i32()
            .ok_or(WasmExecutionError::ExportError(ExportError::IncompatibleType))?;
    }

    // Check that the main function exists and it's signature is correct
    let _main: MainFunction = instance.exports.get_typed_function(store, main_fn)?;

    Ok(())
}

fn validate_functions(template_def: &TemplateDef) -> Result<(), WasmExecutionError> {
    match template_def {
        TemplateDef::V1(def) => {
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

                if func.arguments.len() > limits::WASM_LIMITS.max_function_arguments {
                    return Err(WasmValidationError::FunctionTooManyArguments {
                        name: func.name.clone(),
                        max_args: limits::WASM_LIMITS.max_function_arguments,
                        num_args: func.arguments.len(),
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
                    match &arg.arg_type {
                        Type::Tuple(tuple) if tuple.len() > limits::WASM_LIMITS.max_function_arguments => {
                            return Err(WasmValidationError::FunctionTooManyTupleReturn {
                                name: func.name.clone(),
                                max_tuple_size: limits::WASM_LIMITS.max_function_arguments,
                                tuple_size: tuple.len(),
                            }
                            .into());
                        },
                        Type::Other { name } if name.len() > limits::WASM_LIMITS.max_function_name_length => {
                            return Err(WasmValidationError::FunctionNameTooLong {
                                name: name.clone(),
                                max_length: limits::WASM_LIMITS.max_function_name_length,
                            }
                            .into());
                        },
                        _ => {},
                    }
                }
                if func.is_migration {
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
