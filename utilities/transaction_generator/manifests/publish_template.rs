//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// Generates transactions that each publish the SAME WASM template binary while paying the fee from
// a single given account. A template's on-chain address is H(author_public_key, binary_hash), so
// publishing one binary repeatedly would collide on a duplicate substate. Run this manifest with
// `--random-signer`: the generator seals every transaction with a fresh random keypair (giving each
// publish a distinct author and so a distinct address) and adds the fee-paying account's owner as a
// second signer so it can authorise pay_fee.
//
// The binary is supplied as a manifest blob via `--blob template=<path/to/template.wasm>` and
// referenced below as `publish_template!(template)`. Reuse the max_compute stress template (build
// it with `cargo build --release --target wasm32-unknown-unknown` in templates/max_compute) or
// point at any other WASM template.
//
//   transaction_generator write -n 100 -o publish.bin \
//     -m utilities/transaction_generator/manifests/publish_template.rs \
//     --blob template=<path/to/max_compute.wasm> \
//     --arg account=component_<hex> \
//     --input vault_<hex> \
//     --signer <account_owner_secret_key_hex> \
//     --random-signer
//
// The account is referenced via `arg!["account"]`, so the generator declares it as an input
// automatically. The fee vault is debited by pay_fee but never named in the instructions, so it
// must be declared explicitly with `--input vault_<hex>` — otherwise execution fails with
// SubstateNotFound.

fn fee_main() {
    let account = arg!["account"];
    // pay_fee locks a ceiling and refunds the unused remainder. Publishing costs roughly
    // binary_size µT (storage) + binary_size/3 µT (transaction weight); 1 tTARI covers the small
    // max_compute binary with head-room. Raise it for larger templates (binary cap is 1.5 MiB).
    account.pay_fee(1_000_000);
}

fn main() {
    publish_template!(template);
}
