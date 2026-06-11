//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Stacks several `busy_max()` calls into a single transaction to exercise the per-transaction
// metering budget (`MAX_WASM_POINTS_PER_TRANSACTION`, see crates/engine/src/wasm/process.rs).
//
// Each `busy_max()` consumes ~88M of the 100M per-transaction budget, so the SECOND call runs out of
// gas and the transaction is REJECTED with an execution failure — even though, on a fresh per-call
// budget, every call would have succeeded. This is the "stack many heavy calls in one transaction"
// attack the per-transaction budget is meant to block; expect these transactions to show as
// not-committed / errored in the stress summary.
//
// Same arguments as max_compute.rs:
//
//   transaction_generator write -n 1000 -o stacked.bin \
//     -m utilities/transaction_generator/manifests/max_compute_stacked.rs \
//     --template MaxCompute=template_<hex> \
//     --arg account=component_<hex> \
//     --input vault_<hex> \
//     --signer <account_owner_secret_key_hex>
//
// The fee covers the ~100M points the transaction burns before tripping the budget, so it is
// rejected for running out of gas rather than for underpaying fees.
use MaxCompute;

fn fee_main() {
    let account = arg!["account"];
    account.pay_fee(200_000);
}

fn main() {
    MaxCompute::busy_max();
    MaxCompute::busy_max();
    MaxCompute::busy_max();
    MaxCompute::busy_max();
}
