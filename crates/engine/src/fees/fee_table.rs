//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone)]
pub struct FeeTable {
    pub per_transaction_weight_cost: u64,
    pub per_module_call_cost: u64,
    pub per_byte_storage_cost: u64,
    pub per_event_cost: u64,
    pub per_log_cost: u64,
    pub per_signature_verification_cost: u64,
    pub per_template_load_cost_unit: u64,
    /// Flat cost charged once per newly-created substate, on top of `per_byte_storage_cost`.
    /// Reflects the slot-allocation cost of adding a new entry to permanent state, separate from
    /// the byte cost of its contents.
    pub per_substate_create_cost: u64,
    /// Cost charged per `wasm_points_cost_divisor` Wasmer metering points consumed during template
    /// execution. The opcode → point mapping lives in `wasm/metering.rs` (most ops cost 1 point;
    /// calls 4; heavy ops up to 40). Set to 0 to disable WASM execution metering.
    pub per_wasm_point_cost: u64,
    /// Divisor applied to the raw per-byte storage cost (`per_byte_storage_cost × bytes /
    /// storage_cost_divisor`). Tunes how much of the byte cost is reflected in fees without
    /// changing the rest of the table. Must be non-zero; a zero divisor is treated as `1`.
    pub storage_cost_divisor: u64,
    /// Divisor applied to bytes-loaded when computing template-load units
    /// (`per_template_load_cost_unit × (bytes_loaded / template_load_bytes_cost_divisor)`). Lower
    /// values increase the load fee. Must be non-zero; a zero divisor is treated as `1`.
    pub template_load_bytes_cost_divisor: u64,
    /// Divisor applied to consumed Wasmer points when computing WASM execution units
    /// (`per_wasm_point_cost × (points_consumed / wasm_points_cost_divisor)`). Lower values make
    /// metering more aggressive. Must be non-zero; a zero divisor is treated as `1`.
    pub wasm_points_cost_divisor: u64,
}

impl FeeTable {
    pub fn zero_rated() -> Self {
        Self {
            per_transaction_weight_cost: 0,
            per_module_call_cost: 0,
            per_byte_storage_cost: 0,
            per_event_cost: 0,
            per_log_cost: 0,
            per_signature_verification_cost: 0,
            per_template_load_cost_unit: 0,
            per_substate_create_cost: 0,
            per_wasm_point_cost: 0,
            storage_cost_divisor: 1,
            template_load_bytes_cost_divisor: 1,
            wasm_points_cost_divisor: 1,
        }
    }

    pub fn per_transaction_weight_cost(&self) -> u64 {
        self.per_transaction_weight_cost
    }

    pub fn per_module_call_cost(&self) -> u64 {
        self.per_module_call_cost
    }

    pub fn per_byte_storage_cost(&self) -> u64 {
        self.per_byte_storage_cost
    }

    pub fn per_event_cost(&self) -> u64 {
        self.per_event_cost
    }

    pub fn per_log_cost(&self) -> u64 {
        self.per_log_cost
    }

    pub fn per_signature_verification_cost(&self) -> u64 {
        self.per_signature_verification_cost
    }

    pub fn per_template_load_cost_unit(&self) -> u64 {
        self.per_template_load_cost_unit
    }

    pub fn per_substate_create_cost(&self) -> u64 {
        self.per_substate_create_cost
    }

    pub fn per_wasm_point_cost(&self) -> u64 {
        self.per_wasm_point_cost
    }

    pub fn storage_cost_divisor(&self) -> u64 {
        non_zero(self.storage_cost_divisor)
    }

    pub fn template_load_bytes_cost_divisor(&self) -> u64 {
        non_zero(self.template_load_bytes_cost_divisor)
    }

    pub fn wasm_points_cost_divisor(&self) -> u64 {
        non_zero(self.wasm_points_cost_divisor)
    }
}

fn non_zero(divisor: u64) -> u64 {
    if divisor == 0 { 1 } else { divisor }
}
