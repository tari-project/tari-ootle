// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! Length-aware metering for the bulk memory operators.
//!
//! The static cost function in [`super::metering`] prices one operator at one constant, which is
//! the wrong shape for an operator whose work is a runtime operand: `memory.copy` moves between 0
//! and 4 GiB of bytes for the same instruction. This middleware adds the length-proportional half
//! of the charge, emitted inline against the same remaining-points global the
//! [`tari_wasmer_middlewares::Metering`] middleware installs, so an oversized copy traps on the
//! meter before the copy runs rather than after.
//!
//! It must be pushed *after* `Metering`. Middleware stages run in push order, each consuming the
//! previous stage's output, so pushing second means the metering global indexes are already
//! recorded and the operators emitted here bypass static costing.

use std::sync::{Arc, OnceLock};

use tari_wasmer_middlewares::Metering;
use wasmer::{
    GlobalInit,
    GlobalType,
    LocalFunctionIndex,
    ModuleInfo,
    Mutability,
    Type,
    sys::{FunctionMiddleware, MiddlewareError, MiddlewareReaderState, ModuleMiddleware},
    wasmparser::{BlockType, Operator},
};

use super::metering::CostFunction;

/// Points charged per byte moved by `memory.copy`, `memory.fill` and `memory.init`.
///
/// Metering points are calibrated at ~8.4M points/ms of validator CPU (see
/// [`tari_engine_types::limits::MAX_WASM_POINTS_PER_TRANSACTION`]). A host `memcpy` sustains on the
/// order of 10 GB/s, i.e. ~10M bytes/ms, so cost-neutral pricing is ~0.84 points/byte. Rounded up:
/// a bulk copy is never cheaper per byte than the meter believes.
const POINTS_PER_MEMORY_BYTE: i64 = 1;

/// Indexes of the two globals this middleware appends to every module.
///
/// A middleware cannot add function locals — `locals_info` is read-only — so the operand a charge
/// is computed from is parked in a module global between being popped off the stack and being
/// pushed back. Threads are disabled in the engine's feature set, so a module global is
/// single-writer and this is sound.
#[derive(Debug, Clone, Copy)]
struct ScratchGlobals {
    /// `i32` holding the length operand of the operator being charged.
    len: u32,
    /// `i64` holding the points that length comes to.
    cost: u32,
}

pub struct BulkMetering {
    metering: Arc<Metering<CostFunction>>,
    /// Written once by `transform_module_info`, then read by every
    /// `generate_function_middleware`. Cranelift translates a module's functions across a rayon
    /// pool, so those reads happen on several threads at once and the cell has to be `Sync`;
    /// write-once is what lets them be lock-free.
    scratch: OnceLock<ScratchGlobals>,
}

impl BulkMetering {
    /// `metering` must be the same middleware instance that is pushed before this one; its global
    /// indexes are read when function middlewares are generated.
    pub fn new(metering: Arc<Metering<CostFunction>>) -> Self {
        Self {
            metering,
            scratch: OnceLock::new(),
        }
    }
}

impl std::fmt::Debug for BulkMetering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BulkMetering").finish_non_exhaustive()
    }
}

impl ModuleMiddleware for BulkMetering {
    fn generate_function_middleware<'a>(&self, _: LocalFunctionIndex) -> Box<dyn FunctionMiddleware<'a> + 'a> {
        let indexes = self
            .metering
            .global_indexes()
            .expect("Metering::transform_module_info must run before BulkMetering generates a function middleware");
        let scratch = *self
            .scratch
            .get()
            .expect("BulkMetering::transform_module_info must run before it generates a function middleware");

        Box::new(FunctionBulkMetering {
            remaining_points: indexes.remaining_points().as_u32(),
            points_exhausted: indexes.points_exhausted().as_u32(),
            scratch,
        })
    }

    fn transform_module_info(&self, module_info: &mut ModuleInfo) -> Result<(), MiddlewareError> {
        let len = module_info.globals.push(GlobalType::new(Type::I32, Mutability::Var));
        module_info.global_initializers.push(GlobalInit::I32Const(0));

        let cost = module_info.globals.push(GlobalType::new(Type::I64, Mutability::Var));
        module_info.global_initializers.push(GlobalInit::I64Const(0));

        self.scratch
            .set(ScratchGlobals {
                len: len.as_u32(),
                cost: cost.as_u32(),
            })
            .expect("BulkMetering::transform_module_info: a middleware instance serves exactly one module");

        Ok(())
    }
}

#[derive(Debug)]
struct FunctionBulkMetering {
    remaining_points: u32,
    points_exhausted: u32,
    scratch: ScratchGlobals,
}

/// How an operator's runtime operand turns into a charge.
#[derive(Debug, Clone, Copy)]
pub struct Charge {
    /// Points per unit of the operand on top of the stack.
    rate: i64,
}

/// The charge for an operator whose work is a runtime operand, or `None` for one the static cost
/// table in [`super::metering`] prices in full.
///
/// This is the authority on which operators carry an inline charge: `metering::cost_function` reads
/// it back so the static half and the length-aware half cannot drift apart.
pub fn charge_for(operator: &Operator) -> Option<Charge> {
    match operator {
        // `[dst, src, len]` / `[dst, value, len]` / `[dst, offset, len]`: bytes. An out-of-range
        // length traps on the bounds check, which ends the call, so there is no refusal to price.
        Operator::MemoryCopy { .. } | Operator::MemoryFill { .. } | Operator::MemoryInit { .. } => Some(Charge {
            rate: POINTS_PER_MEMORY_BYTE,
        }),
        // Every table-writing operator, `table.init` among them, is refused at compile (see
        // `instance_reset`), so a table never changes after instantiation.
        _ => None,
    }
}

impl<'a> FunctionMiddleware<'a> for FunctionBulkMetering {
    fn feed(&mut self, operator: Operator<'a>, state: &mut MiddlewareReaderState<'a>) -> Result<(), MiddlewareError> {
        let Some(charge) = charge_for(&operator) else {
            state.push_operator(operator);
            return Ok(());
        };

        // Every operator priced here leaves its count — bytes or elements — on top of the stack, so
        // the charge is computed from the value popped here and the value is pushed back unchanged
        // before the operator runs.
        state.extend([
            Operator::GlobalSet {
                global_index: self.scratch.len,
            },
            Operator::GlobalGet {
                global_index: self.scratch.len,
            },
            Operator::I64ExtendI32U,
            Operator::I64Const { value: charge.rate },
            Operator::I64Mul,
            Operator::GlobalSet {
                global_index: self.scratch.cost,
            },
            // if unsigned(remaining_points) < unsigned(cost) { points_exhausted = 1; trap }
            Operator::GlobalGet {
                global_index: self.remaining_points,
            },
            Operator::GlobalGet {
                global_index: self.scratch.cost,
            },
            Operator::I64LtU,
            Operator::If {
                blockty: BlockType::Empty,
            },
            Operator::I32Const { value: 1 },
            Operator::GlobalSet {
                global_index: self.points_exhausted,
            },
            Operator::Unreachable,
            Operator::End,
            // remaining_points -= cost
            Operator::GlobalGet {
                global_index: self.remaining_points,
            },
            Operator::GlobalGet {
                global_index: self.scratch.cost,
            },
            Operator::I64Sub,
            Operator::GlobalSet {
                global_index: self.remaining_points,
            },
            Operator::GlobalGet {
                global_index: self.scratch.len,
            },
        ]);

        state.push_operator(operator);

        Ok(())
    }
}
