//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Fee tables for Tari DAN transaction execution.
//!
//! This module defines the fee structure for executing transactions on the Tari Digital Asset Network (DAN).
//! Fees are charged based on various resource consumption metrics to incentivize efficient smart contract
//! design and prevent abuse.
//!
//! # Fee Components
//!
//! ## Transaction-Level Fees
//!
//! - **`per_transaction_weight_cost`**: Cost per unit of transaction weight. Transaction weight is calculated based on
//!   the transaction size and complexity.
//!
//! - **`per_module_call_cost`**: Cost charged for each runtime module call (e.g., component method invocations). This
//!   fee applies each time a WASM template function is called during transaction execution.
//!
//! ## Template Loading Fees
//!
//! - **`per_template_load_cost_unit`**: Cost per "load unit" when a template (smart contract WASM code) is loaded from
//!   storage into the execution environment.
//!
//!   **How it's calculated:**
//!   - Template load units = `(bytes_loaded / 3000)` (i.e., 3 KB = 1 unit)
//!   - Total fee = `template_load_units × per_template_load_cost_unit`
//!
//!   **Example:** Loading a 15 KB template:
//!   - Load units = 15,000 / 3,000 = 5 units
//!   - Fee = 5 units × 1 = 5 (for testnet)
//!
//!   This incentivizes compact smart contract code and caches templates that are frequently used.
//!
//! ## Storage Fees
//!
//! - **`per_byte_storage_cost`**: Cost per byte of data written to persistent storage (substates). Note: The actual
//!   cost is reduced by a factor of 4 (cost × 0.25) to make storage more affordable.
//!
//! ## Cryptographic Operation Fees
//!
//! - **`per_signature_verification_cost`**: Cost for each cryptographic signature verification performed during
//!   transaction execution. Signature verification is computationally expensive and thus has a higher relative cost
//!   (10x on testnet).
//!
//! # Fee Timing
//!
//! Fees are assessed at different points during transaction execution:
//! 1. **On Initialize**: Transaction weight costs
//! 2. **During Execution**: Module calls, template loads, signature verifications
//! 3. **Before Finalize**: Storage costs, WASM execution costs

use tari_engine::fees::FeeTable;
use tari_ootle_transaction::Network;

/// Testnet fee table with low, development-friendly fees.
///
/// These values are intentionally set low to facilitate testing and development.
/// Most operations cost 1 unit except signature verification which costs 10 units
/// to reflect its higher computational cost.
const TESTNET_FEE_TABLE: FeeTable = FeeTable {
    per_transaction_weight_cost: 1,
    per_module_call_cost: 1,
    per_byte_storage_cost: 1,
    per_signature_verification_cost: 10,
    // Bumped from 1 to better reflect worst-case cold wasmer instantiation: a 2 MiB template (the
    // current cap) costs ~7000 µT to load vs ~700 µT previously.
    per_template_load_cost_unit: 10,
    // Slot-allocation premium for newly-created substates, on top of `per_byte_storage_cost`.
    // ~25 µT per new substate (~equivalent to 100 bytes of storage at the effective per-byte rate).
    per_substate_create_cost: 25,
    // µT charged per `wasm_points_cost_divisor` Wasmer metering points. Opcode → point mapping
    // lives in `wasm/metering.rs` (most arithmetic = 1, calls = 4, heavy ops up to 40). Starts at
    // 1 µT per 1000 points; revisit once we have hardware-benchmarked numbers.
    per_wasm_point_cost: 1,
    // Divides the raw `per_byte_storage_cost × bytes` storage charge. `1` charges the full byte
    // cost — storage is the only indefinite-liability category, so we don't haircut it further
    // until a proper rent/endowment model lands.
    storage_cost_divisor: 1,
    // 3 KB = 1 template-load unit. Lower values increase the load fee.
    template_load_bytes_cost_divisor: 3000,
    // 1 µT per 1000 Wasmer points. Lower values make metering more aggressive.
    wasm_points_cost_divisor: 1000,
};

/// MainNet fee table - production values.
///
/// # ⚠️ TODO: Finalize MainNet Fee Values
///
/// These values are currently set to the same low rates as testnet but **must be adjusted**
/// before MainNet deployment based on:
/// - Economic modeling and tokenomics
/// - Actual resource costs (CPU, storage, bandwidth)
/// - Network congestion and spam prevention requirements
/// - Competitive analysis with other smart contract platforms
///
/// **Current Status:** Placeholder values - NOT suitable for production use.
// TODO: finalize these values
const MAINNET_FEE_TABLE: FeeTable = FeeTable {
    per_transaction_weight_cost: 1,
    per_module_call_cost: 1,
    per_byte_storage_cost: 1,
    per_signature_verification_cost: 10,
    per_template_load_cost_unit: 10,
    per_substate_create_cost: 25,
    per_wasm_point_cost: 1,
    storage_cost_divisor: 1,
    template_load_bytes_cost_divisor: 3000,
    wasm_points_cost_divisor: 1000,
};

/// Returns the appropriate fee table for the specified network.
///
/// # Networks
///
/// - **Testnet networks** (LocalNet, Igor, Esmeralda, StageNet, NextNet): Use low, development-friendly fees
/// - **MainNet**: Uses production fee table (currently TODO - needs finalization)
///
/// # Example
///
/// ```no_run
/// use tari_ootle_app_utilities::fee_tables::get_fee_table_by_network;
/// use tari_ootle_common_types::Network;
///
/// let fee_table = get_fee_table_by_network(Network::Igor);
/// println!(
///     "Template load cost per unit: {}",
///     fee_table.per_template_load_cost_unit()
/// );
/// ```
pub const fn get_fee_table_by_network(network: Network) -> &'static FeeTable {
    match network {
        Network::LocalNet => &TESTNET_FEE_TABLE,
        Network::Igor => &TESTNET_FEE_TABLE,
        Network::Esmeralda => &TESTNET_FEE_TABLE,
        Network::StageNet => &TESTNET_FEE_TABLE,
        Network::NextNet => &TESTNET_FEE_TABLE,
        Network::MainNet => &MAINNET_FEE_TABLE,
    }
}
