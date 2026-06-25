// Copyright 2022 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_bor::BorError;
use tari_engine_types::indexed_value::IndexedValueError;
use tari_template_abi::{TEMPLATE_DEF_CUSTOM_SECTION, version::WasmAbiVersion};
use wasmer::{ExportError, InstantiationError, MemoryAccessError};

use crate::runtime::RuntimeError;

#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("Missing argument at position {position} (name: {argument_name}")]
    MissingArgument { argument_name: String, position: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum WasmExecutionError {
    #[error(transparent)]
    InstantiationError(Box<InstantiationError>),
    #[error(transparent)]
    ExportError(#[from] ExportError),
    // NOTE this renders as "Wasm RuntimeError: <message>"
    #[error("Wasm {0}")]
    WasmRuntimeError(#[from] wasmer::RuntimeError),
    #[error(
        "Insufficient fees to pay for compute: consumed {consumed_points} WASM metering points, which exceeds what \
         the paid fees cover. Increase the transaction fee."
    )]
    InsufficientFeesForCompute { consumed_points: u64 },
    #[error("Expected function {function} to return a pointer")]
    ExpectedPointerReturn { function: String },
    #[error("Memory access error: {0}")]
    MemoryAccessError(#[from] MemoryAccessError),
    #[error("memory underflow: {required} bytes required but {remaining} remaining")]
    MemoryUnderflow { required: usize, remaining: usize },
    #[error("memory pointer out of range: memory size is {size}, pointer is {pointer} and length is {len}")]
    MemoryPointerOutOfRange { size: u64, pointer: u32, len: u32 },
    #[error("Memory allocation too large")]
    MemoryAllocationTooLarge,
    #[error("Memory allocation failed")]
    MemoryAllocationFailed,
    #[error("BUG: memory not set in environment")]
    MemoryNotSet,
    #[error("Missing function {function}")]
    MissingAbiFunction { function: &'static str },
    #[error("Engine call size limit of {limit} bytes exceeded")]
    CallSizeLimitExceeded { limit: usize },
    #[error("Runtime error: {0}")]
    RuntimeError(#[from] RuntimeError),
    #[error("Failed to decode argument for engine call: {0:?}")]
    EngineArgDecodeFailed(BorError),
    #[error("Failed to decode template definition: {0:?}")]
    AbiTemplateDefDecodeError(BorError),
    #[error("Malformed `tari_tdef` custom section: {reason}")]
    AbiTemplateDefSectionMalformed { reason: String },
    #[error("Unexpected ABI function {name}")]
    UnexpectedAbiFunction { name: String },
    #[error("Encoding error: {0}")]
    EncodingError(#[from] BorError),
    #[error("Panic! {message}")]
    Panic {
        message: String,
        runtime_error: wasmer::RuntimeError,
    },
    #[error("Value visitor error: {0}")]
    ValueVisitorError(#[from] IndexedValueError),
    #[error("Template version {template_version} is incompatible with current engine version {engine_version}")]
    TemplateVersionMismatch {
        engine_version: WasmAbiVersion,
        template_version: WasmAbiVersion,
    },
    #[error("Function '{name}' expected {expected} arguments, but got {actual}")]
    InvalidArgumentCount {
        name: String,
        expected: usize,
        actual: usize,
    },
    #[error("Wasm validation error: {0}")]
    WasmValidationError(#[from] WasmValidationError),
}

#[derive(Debug, thiserror::Error)]
pub enum WasmValidationError {
    #[error("Function name {name} is too long, maximum length is {max_length}")]
    FunctionNameTooLong { name: String, max_length: usize },
    #[error("Function {name} contained too many arguments, maximum is {max_args}, but got {num_args}")]
    FunctionTooManyArguments {
        name: String,
        max_args: usize,
        num_args: usize,
    },
    #[error(
        "Function {name} contained too many return values in a tuple, maximum is {max_tuple_size}, but got \
         {tuple_size}"
    )]
    FunctionTooManyTupleReturn {
        name: String,
        max_tuple_size: usize,
        tuple_size: usize,
    },
    #[error("Too many functions in the module, maximum is {max_functions}")]
    TooManyFunctions { max_functions: usize },
    #[error("Function {function_name} has invalid return type: {return_type:?}")]
    InvalidMigrationReturnType {
        function_name: String,
        return_type: tari_template_abi::Type,
    },
    #[error(
        "Module contains disallowed custom section `{name}`; only `{TEMPLATE_DEF_CUSTOM_SECTION}` is permitted. Strip \
         debug and metadata sections before publishing (e.g. `wasm-opt --strip-debug --strip-producers` or \
         `wasm-strip`)."
    )]
    DisallowedCustomSection { name: String },
}

impl From<wasmer::InstantiationError> for WasmExecutionError {
    fn from(value: InstantiationError) -> Self {
        Self::InstantiationError(Box::new(value))
    }
}
