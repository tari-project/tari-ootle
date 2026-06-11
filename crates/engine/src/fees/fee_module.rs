//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{ByteCounter, encode_into_writer};
use tari_engine_types::fees::FeeSource;
use tari_template_lib::types::TemplateAddress;

use super::FeeTable;
use crate::{
    runtime::{RuntimeEvent, RuntimeModule, RuntimeModuleError, StateTracker},
    state_store::StateReader,
};

pub struct FeeModule {
    initial_cost: u64,
    fee_table: FeeTable,
    dry_run: bool,
}

impl FeeModule {
    pub const fn new(initial_cost: u64, fee_table: FeeTable, dry_run: bool) -> Self {
        Self {
            initial_cost,
            fee_table,
            dry_run,
        }
    }
}

impl<TStore: StateReader> RuntimeModule<TStore> for FeeModule {
    fn on_initialize(&self, track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
        track.set_fee_state_dry_run(self.dry_run);
        track.add_fee_charge(FeeSource::Initial, self.initial_cost);
        let transaction_weight = track.get_transaction_weight();
        let transaction_weight_cost = transaction_weight
            .as_u64()
            .checked_mul(self.fee_table.per_transaction_weight_cost())
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating transaction weight cost".to_string()))?;
        track.add_fee_charge(FeeSource::TransactionWeight, transaction_weight_cost);

        Ok(())
    }

    fn on_runtime_call(&self, track: &mut StateTracker<TStore>, _call: &'static str) -> Result<(), RuntimeModuleError> {
        track.add_fee_charge(FeeSource::RuntimeCall, self.fee_table.per_module_call_cost());
        Ok(())
    }

    fn on_template_loaded(
        &self,
        track: &mut StateTracker<TStore>,
        template_address: &TemplateAddress,
        bytes_loaded: usize,
    ) -> Result<(), RuntimeModuleError> {
        // Dedupe per template per transaction: the validator's compile/deserialise cost is paid
        // once per template per process (in-memory + on-disk caches), so subsequent loads within
        // the same transaction (cross-template calls, repeated method invocations on the same
        // component, etc.) carry no incremental load cost. Per-call dispatch overhead is already
        // captured by `per_module_call_cost`.
        if !track.record_template_load_charge(*template_address) {
            return Ok(());
        }

        let template_load_cost_unit =
            u64::try_from(bytes_loaded).unwrap_or(u64::MAX) / self.fee_table.template_load_bytes_cost_divisor();

        let fee_charge = template_load_cost_unit
            .checked_mul(self.fee_table.per_template_load_cost_unit())
            .ok_or_else(|| {
                RuntimeModuleError::Overflow("Overflow calculating template load weight cost".to_string())
            })?;

        track.add_fee_charge(FeeSource::TemplateLoad, fee_charge);
        Ok(())
    }

    fn on_before_finalize(&self, track: &mut StateTracker<TStore>) -> Result<(), RuntimeModuleError> {
        let total_storage = track.with_substates_to_persist(|changes| {
            let mut counter = ByteCounter::new();
            for substate in changes.values() {
                encode_into_writer(substate, &mut counter)?;
            }
            Ok::<_, RuntimeModuleError>(counter.get())
        })?;

        let cost = self
            .fee_table
            .per_byte_storage_cost()
            .checked_mul(total_storage as u64)
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating storage cost".to_string()))?;
        track.add_fee_charge(FeeSource::Storage, cost / self.fee_table.storage_cost_divisor());

        let new_substate_count = track
            .count_newly_created_substates()
            .map_err(|e| RuntimeModuleError::Runtime(e.to_string()))?;
        let create_cost = (new_substate_count as u64)
            .checked_mul(self.fee_table.per_substate_create_cost())
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating substate create cost".to_string()))?;
        track.add_fee_charge(FeeSource::SubstateCreate, create_cost);

        // WASM execution: charge once against the transaction's accumulated points so the divisor
        // rounds against the total. Per-call rounding would let a transaction split work into
        // sub-divisor chunks and pay zero for any single one (each `points/divisor` is `0`), even
        // though the summed work is non-trivial.
        let units = track.accumulated_wasm_points() / self.fee_table.wasm_points_cost_divisor();
        let wasm_cost = units
            .checked_mul(self.fee_table.per_wasm_point_cost())
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating WASM execution cost".to_string()))?;
        track.add_fee_charge(FeeSource::WasmExecution, wasm_cost);

        Ok(())
    }

    fn on_runtime_event(
        &self,
        track: &mut StateTracker<TStore>,
        call: &RuntimeEvent,
    ) -> Result<(), RuntimeModuleError> {
        match call {
            RuntimeEvent::SignatureVerified => {
                track.add_fee_charge(
                    FeeSource::SignatureVerification,
                    self.fee_table.per_signature_verification_cost(),
                );
            },
        }

        Ok(())
    }
}
