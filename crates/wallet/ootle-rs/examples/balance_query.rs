//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Example demonstrating balance query helpers on the IndexerProvider.
//!
//! This example shows:
//! - `get_account_balance` to query a single resource balance for an account
//! - `get_account_balances` to query all resource balances for an account
//! - `get_utxo_value` to decrypt a stealth UTXO value using an ElGamal view key
//!
//! ## Prerequisites
//! - A running localnet with an indexer
//! - The account must exist and have been funded
//! - For UTXO decryption: the resource must have a view key enabled, and the provided `VIEW_SECRET_KEY_HEX` must be the
//!   secret for the corresponding public key set as the view key on the resource.

use std::str::FromStr;

use ootle_rs::{
    Network,
    ToAccountAddress,
    address,
    crypto::GenerateValueLookup,
    default_indexer_url,
    provider::ProviderBuilder,
    template_types::{ResourceAddress, UtxoAddress, UtxoId, constants::TARI_TOKEN},
};
use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::hex::Hex};

// ---- Configuration ----
// Replace these with real values for your environment.

/// The resource address with a view key enabled (for UTXO decryption).
/// Replace with the actual resource address.
/// For example, a resource with a view key enabled is written in a contract/template as follow:
/// ```rust
/// let address = ResourceBuilder::stealth()
///     .with_view_key(public_view_key)
///     // snip..
///     .build();
/// ```
/// This forces a wallet to generate a correct Elgamal encryption proof that is validated by validators
/// whenever they create a new UTXO for the resource.
/// This validated proof can be "decrypted" (brute force) by the holder of the secret view key.
const RESOURCE_ADDRESS_WITH_VIEW_KEY_ENABLED: &str =
    "resource_0000000000000000000000000000000000000000000000000000000000000000";

/// The ElGamal view secret key hex for the resource above.
/// This is the secret key corresponding to the public key set as the view key on the resource.
const VIEW_SECRET_KEY_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The UTXO ID (commitment bytes as hex) identifying a specific stealth UTXO to decrypt.
const UTXO_ID: &str = "utxo_0000000000000000000000000000000000000000000000000000000000000000";

/// The maximum expected value for brute-force decryption range.
/// Higher values take longer but cover larger balances.
/// Note: Using `GenerateValueLookup` generates value commitments on the fly and is slow.
/// In production, pregenerate a lookup bin file (`cargo install tari_value_lookup_generator`)
/// and use the `MMapValueLookup` (enable the `mmap-value-lookup` feature).
const MAX_EXPECTED_VALUE: u64 = 20_000_000;

#[tokio::main]
async fn main() {
    const NETWORK: Network = Network::LocalNet;

    let indexer_api_url = default_indexer_url(NETWORK);

    // The account whose balances we want to query.
    let account_address = address!(
        "otl_loc_10mc0v2lyy43kldl0ft4c2x5pe7j0ckduv8zej6jgr2z2g9m07fz7gl96ar5wwgu0qu0atmr5tl53ye7n38xr5u7ytlmudq0ruxcau0gge7rxk"
    );
    let account_component = account_address.to_account_address();

    let provider = ProviderBuilder::new()
        .connect(indexer_api_url)
        .await
        .expect("Failed to connect to indexer");

    // ---- Query a single resource balance ----
    println!("Querying TARI balance for {account_component}...");
    let tari_balance = provider
        .get_account_balance(account_component, TARI_TOKEN)
        .await
        .expect("Failed to get account balance");
    println!("TARI balance: {tari_balance}");

    // ---- Query all resource balances ----
    println!("\nQuerying all balances for {account_component}...");
    let all_balances = provider
        .get_account_balances(account_component)
        .await
        .expect("Failed to get account balances");
    if all_balances.is_empty() {
        println!("  (no vaults found)");
    } else {
        for (resource, balance) in &all_balances {
            println!("  {resource}: {balance}");
        }
    }

    // ---- Decrypt a stealth UTXO value ----
    // Skip if placeholder values are still set.
    if VIEW_SECRET_KEY_HEX == "0000000000000000000000000000000000000000000000000000000000000000" {
        println!(
            "\nSkipping UTXO decryption (placeholder config values). Set RESOURCE_ADDRESS_WITH_VIEW_KEY_ENABLED, \
             VIEW_SECRET_KEY_HEX, and UTXO_ID to real values to test."
        );
        return;
    }

    let view_secret_key = RistrettoSecretKey::from_hex(VIEW_SECRET_KEY_HEX).expect("Invalid VIEW_SECRET_KEY_HEX");
    let resource_address = ResourceAddress::from_str(RESOURCE_ADDRESS_WITH_VIEW_KEY_ENABLED)
        .expect("Invalid RESOURCE_ADDRESS_WITH_VIEW_KEY_ENABLED");
    let utxo_id = UtxoId::from_hex(UTXO_ID).expect("Invalid UTXO_ID");
    let utxo_address = UtxoAddress::new(resource_address, utxo_id);
    // ---- Decrypt Stealth UTXO Value ----
    // Use the view secret key to decrypt the value of the stealth output we just created.
    // WARN: Elgamal decryption is a brute-force operation and therefore is slow especially with the GenerateValueLookup
    // which generates value commitments on the fly.
    // In production, pregenerate a lookup bin file (cargo install tari_value_lookup_generator) and use the
    // MMapValueLookup (enable the mmap-value-lookup feature).
    println!("\nDecrypting stealth UTXO value for {utxo_address}...");
    let decrypted = provider
        .get_utxo_value(
            &view_secret_key,
            utxo_address,
            0..=MAX_EXPECTED_VALUE,
            &mut GenerateValueLookup,
        )
        .await
        .expect("Failed to decrypt UTXO value");

    match decrypted {
        Some(value) => println!("Decrypted UTXO value: {value}"),
        None => println!(
            "Could not decrypt UTXO value (not in range 0..={MAX_EXPECTED_VALUE}, or no viewable balance proof)"
        ),
    }
}
