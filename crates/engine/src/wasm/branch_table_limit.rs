//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A compile-time bound on the targets of a `br_table`.
//!
//! Cranelift's aarch64 backend lowers a `br_table` to a conditional branch to the default target
//! followed by the jump table inline. That branch reaches ±1 MiB, so a table of ~262k targets
//! sends it to an attacker-chosen offset instead (CVE-2026-115893). The bound is a compile
//! middleware, so it holds on every load as well as at publish: a stored template is checked each
//! time it is compiled, before Cranelift lowers any `br_table` in it.

use wasmer::{
    LocalFunctionIndex,
    sys::{FunctionMiddleware, MiddlewareError, MiddlewareReaderState, ModuleMiddleware},
    wasmparser::Operator,
};

const MIDDLEWARE_NAME: &str = "branch_table_limit";

/// Compile middleware that refuses a module containing a `br_table` with more than `max_targets`
/// targets, its default excluded.
#[derive(Debug)]
pub struct BranchTableLimit {
    max_targets: usize,
}

impl BranchTableLimit {
    pub fn new(max_targets: usize) -> Self {
        Self { max_targets }
    }
}

impl ModuleMiddleware for BranchTableLimit {
    fn generate_function_middleware<'a>(&self, index: LocalFunctionIndex) -> Box<dyn FunctionMiddleware<'a> + 'a> {
        Box::new(FunctionBranchTableLimit {
            function: index,
            max_targets: self.max_targets,
        })
    }
}

#[derive(Debug)]
struct FunctionBranchTableLimit {
    function: LocalFunctionIndex,
    max_targets: usize,
}

impl<'a> FunctionMiddleware<'a> for FunctionBranchTableLimit {
    fn feed(&mut self, operator: Operator<'a>, state: &mut MiddlewareReaderState<'a>) -> Result<(), MiddlewareError> {
        if let Operator::BrTable { targets } = &operator {
            let len = targets.len() as usize;
            if len > self.max_targets {
                return Err(MiddlewareError::new(
                    MIDDLEWARE_NAME,
                    format!(
                        "defined function {} has a br_table of {len} targets, the maximum is {}",
                        self.function.as_u32(),
                        self.max_targets
                    ),
                ));
            }
        }
        state.push_operator(operator);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_engine_types::limits;
    use wasmer::{Module, Store};

    fn compile(targets: usize) -> Result<Module, wasmer::CompileError> {
        let labels = "0 ".repeat(targets);
        let wat = format!(
            r#"(module
                 (func (export "f") (param i32)
                   (block (br_table {labels}0 (local.get 0)))))"#
        );
        let store = Store::new(super::super::engine::create_engine());
        Module::new(&store, wat::parse_str(&wat).unwrap())
    }

    #[test]
    fn a_br_table_at_the_limit_compiles() {
        compile(limits::WASM_LIMITS.max_br_table_targets).unwrap();
    }

    #[test]
    fn a_br_table_past_the_limit_is_refused() {
        let max = limits::WASM_LIMITS.max_br_table_targets;
        let err = compile(max + 1).unwrap_err().to_string();
        assert!(
            err.contains(&format!("br_table of {} targets, the maximum is {max}", max + 1)),
            "unexpected error: {err}"
        );
    }

    /// The size that miscompiles on aarch64 under Cranelift 0.136.0 is refused on every
    /// architecture before Cranelift lowers it.
    #[test]
    fn a_br_table_that_overflows_the_aarch64_branch_range_is_refused() {
        let err = compile(300_000).unwrap_err().to_string();
        assert!(err.contains("br_table of 300000 targets"), "unexpected error: {err}");
    }
}
