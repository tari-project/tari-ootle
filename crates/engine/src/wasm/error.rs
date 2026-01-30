// Copyright 2022 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_bor::BorError;
use tari_engine_types::indexed_value::IndexedValueError;
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
    #[error("Expected function {function} to return a pointer")]
    ExpectedPointerReturn { function: String },
    #[error("Memory access error: {0}")]
    MemoryAccessError(#[from] MemoryAccessError),
    #[error("memory underflow: {required} bytes required but {remaining} remaining")]
    MemoryUnderflow { required: usize, remaining: usize },
    #[error("memory pointer out of range: memory size of {size} but pointer is {pointer}")]
    MemoryPointerOutOfRange { size: u64, pointer: u64, len: u64 },
    #[error("Memory allocation too large")]
    MemoryAllocationTooLarge,
    #[error("Memory export too large")]
    MemoryExportTooLarge,
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
    #[error("maximum module memory size exceeded")]
    MaxMemorySizeExceeded,
    #[error("Failed to decode ABI: {0:?}")]
    AbiDecodeError(BorError),
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
    #[error("Template version parsing error: {0}")]
    TemplateVersionParsingError(#[from] semver::Error),
    #[error("Template version {template_version} is incompatible with current engine version {engine_version}")]
    TemplateVersionMismatch {
        engine_version: String,
        template_version: String,
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
}

impl From<wasmer::InstantiationError> for WasmExecutionError {
    fn from(value: InstantiationError) -> Self {
        Self::InstantiationError(Box::new(value))
    }
}
