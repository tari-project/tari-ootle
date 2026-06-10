//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Generates transactions that call the `max_compute` stress template (templates/max_compute). Each
// call to `busy_max()` burns ~88M Wasmer metering points — close to the 100M per-call cap — so a
// flood of these transactions stresses transaction execution and consensus under worst-case CPU
// load.
//
// The `MaxCompute` alias is resolved from the generator's template map; supply the published
// address, the fee-paying account, and that account's TARI vault on the command line, e.g.:
//
//   transaction_generator write -n 10000 -o stress.bin \
//     -m utilities/transaction_generator/manifests/max_compute.rs \
//     --template MaxCompute=template_<hex> \
//     --arg account=component_<hex> \
//     --input vault_<hex> \
//     --signer <account_owner_secret_key_hex>
//
// The account is referenced below via `arg!["account"]`, so the generator declares it as an input
// automatically. The fee vault is debited by pay_fee but never named in the instructions, so it
// must be declared explicitly with `--input vault_<hex>` — otherwise execution fails with
// SubstateNotFound.
//
// At the testnet fee rate (1 µT per 1000 points) busy_max() costs ~88_000 µT in execution fees, so
// the fee paid below leaves head-room for the template-load and per-call charges.
use MaxCompute;

fn fee_main() {
    let account = arg!["account"];
    account.pay_fee(200_000);
}

fn main() {
    MaxCompute::busy_max();
}
