//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{ByteCounter, encode_into_writer};
use tari_engine_types::{fees::FeeSource, substate::SubstateValue};
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

    /// Raw storage metrics for persisting a published template's `binary_len`-byte binary: the
    /// bytes priced at the per-byte storage rate (the first `template_size_premium_free_bytes`, so
    /// small templates cost the same as ordinary storage) and the quadratic premium on each whole
    /// unit of the excess above that. Returned raw — not pre-divided — so the caller can accumulate
    /// across the transaction and apply `storage_cost_divisor` once at finalization, matching how
    /// the other byte/point charges round against per-transaction totals.
    fn template_publish_metrics(&self, binary_len: usize) -> Result<(u64, u64), RuntimeModuleError> {
        let size = binary_len as u64;
        let free = self.fee_table.template_size_premium_free_bytes();

        let base_bytes = size.min(free);

        let units = size.saturating_sub(free) / self.fee_table.template_size_premium_unit_bytes();
        let premium = units
            .checked_mul(units)
            .and_then(|squared| squared.checked_mul(self.fee_table.per_template_size_premium_unit_cost()))
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating template publish premium".to_string()))?;

        Ok((base_bytes, premium))
    }

    #[cfg(test)]
    fn template_publish_cost(&self, binary_len: usize) -> Result<u64, RuntimeModuleError> {
        let (base_bytes, premium) = self.template_publish_metrics(binary_len)?;
        let base = self
            .fee_table
            .per_byte_storage_cost()
            .checked_mul(base_bytes)
            .ok_or_else(|| {
                RuntimeModuleError::Overflow("Overflow calculating template base storage cost".to_string())
            })? /
            self.fee_table.storage_cost_divisor();
        base.checked_add(premium)
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating template publish cost".to_string()))
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
        let (total_storage, template_base_bytes, template_premium) = track.with_substates_to_persist(|changes| {
            let mut counter = ByteCounter::new();
            let mut base_bytes = 0u64;
            let mut premium = 0u64;
            for substate in changes.values() {
                // A published template's binary is priced by the dedicated base + quadratic publish
                // model, so keep it out of the flat per-byte storage tally. Accumulate the raw
                // metrics here and apply the storage divisor once below.
                if let SubstateValue::Template(template) = substate {
                    let (tpl_base_bytes, tpl_premium) = self.template_publish_metrics(template.binary.len())?;
                    base_bytes = base_bytes.checked_add(tpl_base_bytes).ok_or_else(|| {
                        RuntimeModuleError::Overflow("Overflow accumulating template base bytes".to_string())
                    })?;
                    premium = premium.checked_add(tpl_premium).ok_or_else(|| {
                        RuntimeModuleError::Overflow("Overflow accumulating template publish premium".to_string())
                    })?;
                    continue;
                }
                encode_into_writer(substate, &mut counter)?;
            }
            Ok::<_, RuntimeModuleError>((counter.get(), base_bytes, premium))
        })?;

        let cost = self
            .fee_table
            .per_byte_storage_cost()
            .checked_mul(total_storage as u64)
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating storage cost".to_string()))?;
        track.add_fee_charge(FeeSource::Storage, cost / self.fee_table.storage_cost_divisor());

        let template_base_cost = self
            .fee_table
            .per_byte_storage_cost()
            .checked_mul(template_base_bytes)
            .ok_or_else(|| {
                RuntimeModuleError::Overflow("Overflow calculating template base storage cost".to_string())
            })? /
            self.fee_table.storage_cost_divisor();
        let template_publish_cost = template_base_cost
            .checked_add(template_premium)
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating template publish cost".to_string()))?;
        if template_publish_cost > 0 {
            track.add_fee_charge(FeeSource::TemplatePublish, template_publish_cost);
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn fee_table() -> FeeTable {
        FeeTable {
            per_byte_storage_cost: 1,
            storage_cost_divisor: 1,
            template_size_premium_free_bytes: 30 * 1024,
            template_size_premium_unit_bytes: 1024,
            per_template_size_premium_unit_cost: 100,
            ..FeeTable::zero_rated()
        }
    }

    fn publish_cost(binary_len: usize) -> u64 {
        FeeModule::new(0, fee_table(), false)
            .template_publish_cost(binary_len)
            .unwrap()
    }

    #[test]
    fn at_or_below_the_free_allowance_is_linear() {
        assert_eq!(publish_cost(20 * 1024), 20 * 1024);
        assert_eq!(publish_cost(30 * 1024), 30 * 1024);
    }

    #[test]
    fn above_the_free_allowance_adds_a_quadratic_premium() {
        // base = first 30 KiB linear; premium = units² × 100, units = excess_KiB.
        assert_eq!(publish_cost(38 * 1024), 30 * 1024 + 8 * 8 * 100);
        assert_eq!(publish_cost(64 * 1024), 30 * 1024 + 34 * 34 * 100);
    }

    #[test]
    fn cost_is_monotonic_across_the_threshold() {
        assert!(publish_cost(30 * 1024) <= publish_cost(31 * 1024));
        assert!(publish_cost(31 * 1024) <= publish_cost(32 * 1024));
        assert!(publish_cost(32 * 1024) < publish_cost(64 * 1024));
    }
}
