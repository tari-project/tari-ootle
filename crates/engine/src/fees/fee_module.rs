//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io;

use tari_bor::encode_into_std_writer;
use tari_engine_types::fees::FeeSource;

use super::FeeTable;
use crate::runtime::{RuntimeModule, RuntimeModuleError, StateTracker};

pub struct FeeModule {
    initial_cost: u64,
    fee_table: FeeTable,
}

impl FeeModule {
    pub fn new(initial_cost: u64, fee_table: FeeTable) -> Self {
        Self {
            initial_cost,
            fee_table,
        }
    }
}

impl RuntimeModule for FeeModule {
    fn on_initialize(&self, track: &StateTracker) -> Result<(), RuntimeModuleError> {
        track.add_fee_charge(FeeSource::Initial, self.initial_cost);
        let transaction_weight = track.get_transaction_weight();
        let transaction_weight_cost = transaction_weight.as_u64() * self.fee_table.per_transaction_weight_cost();
        track.add_fee_charge(FeeSource::TransactionWeight, transaction_weight_cost);

        Ok(())
    }

    fn on_runtime_call(&self, track: &StateTracker, _call: &'static str) -> Result<(), RuntimeModuleError> {
        track.add_fee_charge(FeeSource::RuntimeCall, self.fee_table.per_module_call_cost());
        Ok(())
    }

    fn on_before_finalize(&self, track: &StateTracker) -> Result<(), RuntimeModuleError> {
        let total_storage = track.with_substates_to_persist(|changes| {
            let mut counter = ByteCounter::new();
            for substate in changes.values() {
                encode_into_std_writer(substate, &mut counter)?;
            }
            Ok::<_, RuntimeModuleError>(counter.get())
        })?;

        let cost = self
            .fee_table
            .per_byte_storage_cost()
            .checked_mul(total_storage)
            .ok_or_else(|| RuntimeModuleError::Overflow("Overflow calculating storage cost".to_string()))?;
        // TODO: Cost per byte of storage is reduced by a pretty arbitrarily chosen factor (floor(cost*0.25))
        const STORAGE_COST_REDUCTION_DIVISOR: u64 = 4;
        track.add_fee_charge(
            FeeSource::Storage,
            // Divide a storage cost reduction factor
            cost / STORAGE_COST_REDUCTION_DIVISOR,
        );

        track.add_fee_charge(FeeSource::Logs, track.num_logs() as u64 * self.fee_table.per_log_cost());

        track.add_fee_charge(
            FeeSource::Events,
            track.num_events() as u64 * self.fee_table.per_event_cost(),
        );

        Ok(())
    }
}

// TODO: This may become available in tari_utilities in future
#[derive(Debug, Clone, Default)]
struct ByteCounter {
    count: u64,
}

impl ByteCounter {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn get(&self) -> u64 {
        self.count
    }
}

impl io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len();
        let len_u64 = u64::try_from(len).map_err(|_| {
            // Not really possible
            io::Error::other("usize len Overflow u64")
        })?;
        self.count = self
            .count
            .checked_add(len_u64)
            .ok_or_else(|| io::Error::other("ByteCounter overflowed u64"))?;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
