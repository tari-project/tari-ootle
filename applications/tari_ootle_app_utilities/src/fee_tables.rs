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
//! - **`per_byte_storage_cost`**: Cost per byte of data written to persistent storage (substates), divided by
//!   `storage_cost_divisor`. Both shipped tables set that divisor to 1: storage is the only indefinite-liability
//!   category, so the full byte cost stands until a rent or endowment model lands.
//!
//! - **`log_bytes_cost_divisor`**: Divides the per-byte storage rate when charging for a log message
//!   (`per_byte_storage_cost × message_bytes / log_bytes_cost_divisor`). A log is retained in every validator's
//!   execution record but is not consensus state and is prunable, so it is priced below a persisted byte.
//!
//! ## Template Publishing Fees
//!
//! A published template's binary is priced by its own model instead of the flat per-byte storage rate, since it is
//! the largest single payload a caller can commit to permanent state:
//!
//! - `per_template_publish_cost` — flat, charged once per publish.
//! - `template_size_premium_free_bytes` — priced at `per_byte_storage_cost`, no premium.
//! - `per_template_size_premium_unit_cost × units²` — where `units = (size − free) / template_size_premium_unit_bytes`.
//!
//! The quadratic term is what makes oversized templates expensive; the flat term sets the floor, because a small
//! template's size is dominated by fixed `template_lib` machinery rather than by anything its author wrote.
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
    // A log byte costs a 64th of a persisted byte. A log is retained in each validator's record of
    // the execution rather than in consensus state, so it is priced well under permanent storage:
    // a max-size entry (32 KiB) is ~512 µT and a transaction that fills `max_logs` with them is
    // ~0.13 tTARI, against ~8.4 tTARI for the same bytes in a substate. An ordinary diagnostic line
    // costs about as much as the host call that emits it.
    log_bytes_cost_divisor: 64,
    // First 96 KiB of a template binary are priced at the per-byte storage rate; beyond that the
    // quadratic publish premium applies. A published template carries ~25 KiB of fixed
    // `template_lib` machinery before any author code, and a minimal component template optimised
    // the way the publish path optimises it lands around 76 KiB, so the allowance sits just above
    // the floor: the premium prices author content, not library overhead.
    template_size_premium_free_bytes: 96 * 1024,
    // 1 KiB per premium unit.
    template_size_premium_unit_bytes: 1024,
    // 100 µT per unit². e.g. a 256 KiB template (160 units over the free allowance) pays
    // 160² × 100 ≈ 2.6 tTARI premium; a 512 KiB template pays ≈ 17.3 tTARI.
    per_template_size_premium_unit_cost: 100,
    // 250_000 µT flat per publish. The size premium alone cannot price a small template above the
    // noise floor, since most of a small binary is library overhead rather than author content.
    per_template_publish_cost: 250_000,
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
    log_bytes_cost_divisor: 64,
    template_size_premium_free_bytes: 96 * 1024,
    template_size_premium_unit_bytes: 1024,
    per_template_size_premium_unit_cost: 100,
    per_template_publish_cost: 250_000,
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

#[cfg(test)]
mod tests {
    use tari_engine_types::fees::{FEE_ESTIMATE_ALLOWANCE, MAX_EXHAUST_BURN_RATE_BPS};

    use super::*;

    /// `FEE_ESTIMATE_ALLOWANCE` is restated in `tari_engine_types`, which cannot see a `FeeTable`.
    /// Every shipped table must come in under it at the highest burn the estimate is derived
    /// against, or a dry run under-states what a real submission costs.
    #[test]
    fn fee_estimate_allowance_covers_every_shipped_network() {
        for network in [
            Network::MainNet,
            Network::StageNet,
            Network::NextNet,
            Network::Igor,
            Network::Esmeralda,
            Network::LocalNet,
        ] {
            let derived = get_fee_table_by_network(network).fee_estimate_allowance(MAX_EXHAUST_BURN_RATE_BPS);
            assert!(
                derived <= FEE_ESTIMATE_ALLOWANCE,
                "{network} needs an allowance of {derived}, above the restated {FEE_ESTIMATE_ALLOWANCE}"
            );
        }
    }
}
