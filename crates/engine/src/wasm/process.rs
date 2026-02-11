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

use log::*;
use serde::{Serialize, de::DeserializeOwned};
use tari_bor::{ByteCounter, decode_exact, encode_into_writer, encoded_len};
use tari_engine_types::{indexed_value::IndexedValue, instruction_result::InstructionResult, limits};
use tari_template_abi::{CallInfo, EngineOp, FunctionDef, TemplateDef, func_hasher::hash_function_name, version};
use tari_template_lib::{
    args::{
        AddressAllocationInvokeArg,
        BucketInvokeArg,
        BuiltinTemplateInvokeArg,
        CallInvokeArg,
        CallerContextInvokeArg,
        ComponentInvokeArg,
        ConsensusInvokeArg,
        EmitEventArg,
        EmitLogArg,
        GenerateRandomInvokeArg,
        NonFungibleInvokeArg,
        ProofInvokeArg,
        ResourceInvokeArg,
        VaultInvokeArg,
    },
    types::{LogLevel, engine_args::SignatureInvokeArg},
};
use wasmer::{AsStoreMut, Function, FunctionEnv, FunctionEnvMut, Instance, Store, StoreMut, WasmPtr, imports};

use crate::{
    runtime::Runtime,
    traits::Invokable,
    wasm::{
        LoadedWasmTemplate,
        environment::{AllocPtr, WasmEnv},
        error::WasmExecutionError,
        mem_writer::MemWriter,
        module::MainFunction,
    },
};

const LOG_TARGET: &str = "tari::ootle::engine::wasm::process";

pub struct WasmProcess {
    module: LoadedWasmTemplate,
    env: WasmEnv<Runtime>,
    instance: Instance,
}

impl WasmProcess {
    pub fn init(store: &mut Store, module: LoadedWasmTemplate, state: Runtime) -> Result<Self, WasmExecutionError> {
        let mut env = WasmEnv::new(state);
        let fn_env = FunctionEnv::new(store, env.clone());
        let tari_engine = Function::new_typed_with_env(store, &fn_env, Self::tari_engine_entrypoint);

        let imports = imports! {
            "env" => {
                "tari_engine" => tari_engine,
                "tari_debug" => Function::new_typed_with_env(store, &fn_env, debug_handler),
                "on_panic" => Function::new_typed_with_env(store,&fn_env, on_panic_handler),
            }
        };
        let instance = Instance::new(store, module.wasm_module(), &imports)?;
        let memory = instance.exports.get_memory("memory")?.clone();
        let tari_alloc = instance.exports.get_typed_function(store, "tari_alloc")?;
        let tari_free = instance.exports.get_typed_function(store, "tari_free")?;
        fn_env
            .as_mut(store)
            .set_memory(memory.clone())
            .set_alloc_funcs(tari_alloc.clone(), tari_free.clone());

        // Also set these for the local copy
        env.set_memory(memory).set_alloc_funcs(tari_alloc, tari_free);

        Ok(Self { module, env, instance })
    }

    fn with_alloc_and_mem_writer<S, F, R>(
        &self,
        store: &mut S,
        alloc_size: usize,
        callback: F,
    ) -> Result<AllocPtr, WasmExecutionError>
    where
        S: AsStoreMut,
        F: for<'m> Fn(&'m mut MemWriter<'_>) -> Result<R, WasmExecutionError>,
    {
        if alloc_size > limits::ENGINE_LIMITS.max_call_size {
            return Err(WasmExecutionError::CallSizeLimitExceeded {
                limit: limits::ENGINE_LIMITS.max_call_size,
            });
        }
        let len = u32::try_from(alloc_size).map_err(|_| WasmExecutionError::MemoryAllocationTooLarge)?;

        let ptr = self.env.alloc(store, len)?;
        if ptr.is_null() {
            return Err(WasmExecutionError::MemoryAllocationFailed);
        }
        let mut writer = self.env.memory_writer(store, ptr)?;
        callback(&mut writer)?;

        Ok(AllocPtr::new(ptr.offset(), len))
    }

    #[allow(clippy::too_many_lines)]
    fn tari_engine_entrypoint(
        mut env: FunctionEnvMut<WasmEnv<Runtime>>,
        op: i32,
        arg_ptr: WasmPtr<u8>,
        arg_len: u32,
    ) -> WasmPtr<u8> {
        let op = match EngineOp::from_i32(op) {
            Some(op) => op,
            None => {
                log::error!(target: LOG_TARGET, "Invalid opcode: {}", op);
                return WasmPtr::null();
            },
        };

        if arg_len as usize > limits::ENGINE_LIMITS.max_internal_call_size {
            log::error!(
                target: LOG_TARGET,
                "Engine call size limit of {} bytes exceeded: {} bytes",
                limits::ENGINE_LIMITS.max_internal_call_size,
                arg_len
            );
            return WasmPtr::null();
        }

        let (env_mut, store) = env.data_and_store_mut();

        log::debug!(target: LOG_TARGET, "Engine call: {:?}", op);

        let result = match op {
            EngineOp::EmitLog => Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: EmitLogArg| {
                state.interface_mut().emit_log(arg.level, arg.message)
            }),
            EngineOp::ComponentInvoke => {
                Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: ComponentInvokeArg| {
                    state
                        .interface_mut()
                        .component_invoke(arg.component_ref, arg.action, arg.args.into())
                })
            },
            EngineOp::ResourceInvoke => {
                Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: ResourceInvokeArg| {
                    state
                        .interface_mut()
                        .resource_invoke(arg.resource_ref, arg.action, arg.args.into())
                })
            },
            EngineOp::VaultInvoke => Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: VaultInvokeArg| {
                state
                    .interface_mut()
                    .vault_invoke(arg.vault_ref, arg.action, arg.args.into())
            }),
            EngineOp::BucketInvoke => Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: BucketInvokeArg| {
                state
                    .interface_mut()
                    .bucket_invoke(arg.bucket_ref, arg.action, arg.args.into())
            }),
            EngineOp::NonFungibleInvoke => {
                Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: NonFungibleInvokeArg| {
                    state
                        .interface_mut()
                        .non_fungible_invoke(arg.address, arg.action, arg.args.into())
                })
            },
            EngineOp::GenerateUniqueId => Self::handle(store, env_mut, arg_ptr, arg_len, |state, _arg: ()| {
                state.interface_mut().generate_uuid()
            }),
            EngineOp::ConsensusInvoke => {
                Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: ConsensusInvokeArg| {
                    state.interface_mut().consensus_invoke(arg.action)
                })
            },
            EngineOp::CallerContextInvoke => Self::handle(
                store,
                env_mut,
                arg_ptr,
                arg_len,
                |state, arg: CallerContextInvokeArg| {
                    state.interface_mut().caller_context_invoke(arg.action, arg.args.into())
                },
            ),
            EngineOp::AddressAllocationInvoke => Self::handle(
                store,
                env_mut,
                arg_ptr,
                arg_len,
                |state, arg: AddressAllocationInvokeArg| state.interface_mut().allocate_address_invoke(arg),
            ),
            EngineOp::GenerateRandomInvoke => Self::handle(
                store,
                env_mut,
                arg_ptr,
                arg_len,
                |state, arg: GenerateRandomInvokeArg| state.interface_mut().generate_random_invoke(arg.action),
            ),
            EngineOp::EmitEvent => Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: EmitEventArg| {
                state.interface_mut().emit_event(arg.topic, arg.payload)
            }),
            EngineOp::CallInvoke => Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: CallInvokeArg| {
                state.interface_mut().call_invoke(arg.action, arg.args.into())
            }),
            EngineOp::ProofInvoke => Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: ProofInvokeArg| {
                state
                    .interface_mut()
                    .proof_invoke(arg.proof_ref, arg.action, arg.args.into())
            }),
            EngineOp::BuiltinTemplateInvoke => Self::handle(
                store,
                env_mut,
                arg_ptr,
                arg_len,
                |state, arg: BuiltinTemplateInvokeArg| state.interface_mut().builtin_template_invoke(arg.action),
            ),
            EngineOp::SignatureInvoke => {
                Self::handle(store, env_mut, arg_ptr, arg_len, |state, arg: SignatureInvokeArg| {
                    state.interface_mut().signature_invoke(arg.action, arg.args.into())
                })
            },
        };

        result.unwrap_or_else(|err| {
            if let Err(err) = env
                .data_mut()
                .state_mut()
                .interface_mut()
                .emit_log(LogLevel::Error, format!("Execution error: {}", err))
            {
                log::error!(target: LOG_TARGET, "Error emitting log: {}", err);
            }

            log::error!(target: LOG_TARGET, "{}", err);
            if let WasmExecutionError::RuntimeError(e) = err {
                env.data_mut().set_last_engine_error(e);
            }
            WasmPtr::null()
        })
    }

    pub fn handle<T, U, E>(
        mut store: StoreMut,
        env_mut: &mut WasmEnv<Runtime>,
        arg_ptr: WasmPtr<u8>,
        arg_len: u32,
        f: fn(&mut Runtime, T) -> Result<U, E>,
    ) -> Result<WasmPtr<u8>, WasmExecutionError>
    where
        T: DeserializeOwned,
        U: Serialize,
        WasmExecutionError: From<E>,
    {
        // SAFETY: WasmProcess is not used concurrently and templates are not able to spawn threads
        let decoded = unsafe {
            env_mut.with_memory_slice(&mut store, arg_ptr, arg_len, |arg| {
                decode_exact(arg).map_err(|e| {
                    log::error!(target: LOG_TARGET, "Failed to decode args for engine call: {}", e);
                    WasmExecutionError::EngineArgDecodeFailed(e)
                })
            })
        }??;
        let resp = f(env_mut.state_mut(), decoded)?;
        let len = encoded_len(&resp)?;
        let ptr = env_mut.alloc(&mut store, len as u32)?;
        // Encode response directly into the WASM memory. The WASM code is responsible for freeing it.
        let mut writer = env_mut.memory_writer(&mut store, ptr)?;
        encode_into_writer(&resp, &mut writer)?;
        Ok(ptr)
    }

    /// Determine if the version of the template_lib crate in the WASM is valid.
    pub fn validate_template_abi_version(template_def: &TemplateDef) -> Result<(), WasmExecutionError> {
        let template_abi_ver = template_def.abi_version();

        // Remove once minimum supported version is > 0
        #[expect(clippy::absurd_extreme_comparisons)]
        if template_abi_ver >= version::MINIMUM_SUPPORTED_WASM_ABI_VERSION {
            log::debug!(target: LOG_TARGET, "The WASM ABI version (\"{}\") is compatible with the one used in the engine", template_abi_ver);
        } else {
            log::error!(target: LOG_TARGET, "The WASM ABI version (\"{}\") is incompatible with the one used in the engine (\"{}\")", template_abi_ver, version::MINIMUM_SUPPORTED_WASM_ABI_VERSION);
            return Err(WasmExecutionError::TemplateVersionMismatch {
                engine_version: version::MINIMUM_SUPPORTED_WASM_ABI_VERSION,
                template_version: template_abi_ver,
            });
        }

        Ok(())
    }
}

impl Invokable<Store> for WasmProcess {
    type Error = WasmExecutionError;

    fn invoke(
        &mut self,
        store: &mut Store,
        func_def: &FunctionDef,
        args: &[tari_bor::Value],
    ) -> Result<InstructionResult, Self::Error> {
        let main_name = format!("{}_main", self.module.template_name());
        let func: MainFunction = self.instance.exports.get_typed_function(store, &main_name)?;
        if func_def.arguments.len() != args.len() {
            return Err(WasmExecutionError::InvalidArgumentCount {
                name: func_def.name.clone(),
                expected: func_def.arguments.len(),
                actual: args.len(),
            });
        }

        let func_ident = hash_function_name(&func_def.name);
        let mut counter = ByteCounter::new();
        CallInfo::encode_v1_packed(&mut counter, func_ident, args)?;
        let call_info_size = counter.get();
        let call_info_ptr = self.with_alloc_and_mem_writer(store, call_info_size, |mem_writer| {
            CallInfo::encode_v1_packed(mem_writer, func_ident, args)?;
            Ok(())
        })?;

        // Call the contract entrypoint
        let res = func.call(store, call_info_ptr.as_wasm_ptr(), call_info_ptr.len());

        match res {
            Ok(return_ptr) => {
                // Read response from memory
                // SAFETY: WasmProcess is not used concurrently
                let value = unsafe {
                    self.env
                        .with_memory_embedded_len(store, return_ptr.offset(), IndexedValue::from_raw)??
                };

                // Free allocated memory containing the result
                self.env.free(store, return_ptr)?;

                self.env.state().interface().validate_return_value(&value)?;
                self.env
                    .state_mut()
                    .interface_mut()
                    .set_last_instruction_output(value.clone())?;

                Ok(InstructionResult {
                    indexed: value,
                    return_type: func_def.output.clone(),
                })
            },
            Err(err) => {
                if let Some(err) = self.env.take_last_engine_error() {
                    return Err(WasmExecutionError::RuntimeError(err));
                }
                if let Some(message) = self.env.take_last_panic_message() {
                    return Err(WasmExecutionError::Panic {
                        message,
                        runtime_error: err,
                    });
                }
                error!(target: LOG_TARGET, "Error calling function: {}", err);
                Err(err.into())
            },
        }
    }
}

fn debug_handler<T: Send + 'static>(mut env: FunctionEnvMut<WasmEnv<T>>, arg_ptr: WasmPtr<u8>, arg_len: u32) {
    const WASM_DEBUG_LOG_TARGET: &str = "tari::ootle::wasm";
    let (state, mut store) = env.data_and_store_mut();

    // SAFETY: WasmProcess is not used concurrently
    unsafe {
        if let Err(err) = state.with_memory_slice(&mut store, arg_ptr, arg_len, |msg| {
            eprintln!("DEBUG: {}", String::from_utf8_lossy(msg));
        }) {
            log::error!(target: WASM_DEBUG_LOG_TARGET, "Failed to read from memory: {}", err);
        }
    }
}

fn on_panic_handler<T: Send + 'static>(
    mut env: FunctionEnvMut<WasmEnv<T>>,
    msg_ptr: WasmPtr<u8>,
    msg_len: i32,
    line: i32,
    col: i32,
) {
    const WASM_DEBUG_LOG_TARGET: &str = "tari::ootle::wasm";
    let (state, mut store) = env.data_and_store_mut();

    // SAFETY: There is no way to call this function concurrently
    unsafe {
        state
            .with_memory_slice(&mut store, msg_ptr, msg_len as u32, |msg_bytes| {
                if msg_bytes.len() > limits::ENGINE_LIMITS.max_panic_message_size {
                    let Ok(msg) = str::from_utf8(msg_bytes) else {
                        error!(target: WASM_DEBUG_LOG_TARGET, "📣 PANIC: ({}:{}) <invalid utf8 message>", line, col);
                        return;
                    };
                    log::error!(target: WASM_DEBUG_LOG_TARGET, "📣 PANIC: ({}:{}) {}", line, col, msg);
                    let limit = limits::ENGINE_LIMITS.max_panic_message_size;
                    let mut end = limit;
                    // Ensure we truncate at a char boundary (to avoid a panic when calling truncate)
                    while end > 0 && !msg.is_char_boundary(end) {
                        end -= 1;
                    }
                    error!(target: LOG_TARGET, "Panic message size limit exceeded: for panic {}", msg);
                    state.set_last_panic(msg[..end].to_string());
                } else {
                    let msg = String::from_utf8_lossy(msg_bytes);
                    log::error!(target: WASM_DEBUG_LOG_TARGET, "📣 PANIC: ({}:{}) {}", line, col, msg);
                    state.set_last_panic(msg.into_owned());
                }
            })
            .unwrap_or_else(|err| {
                log::error!(
                    target: WASM_DEBUG_LOG_TARGET,
                    "📣 PANIC: WASM template panicked but did not provide a valid memory pointer to on_panic \
                     callback: {}",
                    err
                );
                state.set_last_panic(format!(
                    "WASM panicked but did not provide a valid message pointer to on_panic callback: {}",
                    err
                ));
            });
    }
}
