//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Example: Invoking methods on an arbitrary template using `ootle_template!`
//!
//! This example demonstrates how to define a typed interface for a deployed template
//! (in this case a stable coin) and invoke its methods with compile-time type safety.
//!
//! Before running, you need:
//! - A running local Tari network with indexer
//! - The stable coin template deployed (or update the template address)
//! - Update the component address below if calling component methods

use std::str::FromStr;

use ootle_rs::{
    Network,
    TransactionRequest,
    builtin_templates::{
        UnsignedTransactionBuilder,
        component::{IComponent, OotleInvoke},
        faucet::IFaucet,
    },
    default_indexer_url,
    displayable::Displayable,
    key_provider::PrivateKeyProvider,
    ootle_template,
    provider::{PendingTransaction, ProviderBuilder, WalletProvider},
    template_types::{Amount, ComponentAddress, constants::TARI},
    transaction::TransactionSigner,
    wallet::OotleWallet,
};
use tari_ootle_common_types::engine_types::published_template::PublishedTemplateAddress;
// ---------------------------------------------------------------------------
// Step 1: Define the template interface
//
// The macro generates a single generic struct `StableCoin<'a, P, I>` parameterized
// by an interface marker. Use the constructors to select the interface:
//   - `StableCoin::for_component(addr, &provider)` — component methods (&self / &mut self)
//   - `StableCoin::for_template(addr, &provider)`  — template functions (no self, e.g. constructors)
//
// The method signatures should match the template's public API. Argument types
// must implement `serde::Serialize` for CBOR encoding.
// ---------------------------------------------------------------------------

ootle_template! {
    template StableCoin {
        // Template function (no self) — callable via StableCoinTemplate
        // This is the constructor that instantiates the template into a component.
        fn instantiate(
            view_key: RistrettoPublicKeyBytes
        );

        // Component methods (with self) — callable via StableCoin
        // Supply management
        fn increase_supply(&mut self, amount: Amount);
        fn decrease_supply(&mut self, amount: Amount);

        // Token movement
        fn withdraw(&mut self, amount: Amount);
        fn deposit(&mut self, bucket: Bucket);

        // Admin operations
        fn blacklist_user(&mut self, vault_id: VaultId, user_id: UserId);
        fn remove_from_blacklist(&mut self, user_id: UserId);

        // Configuration
        fn set_config_transfer_fee_fixed(&mut self, new_fee: Amount);
        fn set_config_transfer_fee_percentage(&mut self, new_fee_perc: u8);

        // Compliance
        fn pause(&mut self);
        fn freeze_utxos(&self, utxos: Vec<ootle_rs::template_types::UtxoId>);
        fn unfreeze_utxos(&self, utxos: Vec<ootle_rs::template_types::UtxoId>);
    }
}

// ---------------------------------------------------------------------------
// Step 2: Set your deployment addresses
//
// Replace these with your actual deployed addresses.
// ---------------------------------------------------------------------------

#[expect(clippy::too_many_lines)]
#[tokio::main]
async fn main() {
    const NETWORK: Network = Network::LocalNet;

    // The template address of the deployed stable coin template.
    let stable_coin_template =
        PublishedTemplateAddress::from_str("template_6ab6a67645c41fdab217de0dcf675ae2013c7961ef41e426367fee298702c45f")
            .unwrap();
    // The component address of an instantiated stable coin (if calling methods).
    let stable_coin_component =
        ComponentAddress::from_str("component_2a0faf64a52c96fddf4fa300322ab691671c51d0ad65151327e3e70a45f9aae3")
            .unwrap();

    let indexer_api_url = default_indexer_url(NETWORK);

    // Create wallet and provider
    let sender_secret = PrivateKeyProvider::random(NETWORK);
    let sender_address = sender_secret.address().clone();
    println!("Sender address: {sender_address}");

    let wallet = OotleWallet::from(sender_secret);
    let mut provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(indexer_api_url)
        .await
        .expect("Failed to connect to indexer");

    // Fund the account from faucet
    let unsigned_tx = IFaucet::new(&provider)
        .take_faucet_funds(10 * TARI)
        .pay_fee(500u64)
        .prepare()
        .await
        .expect("Failed to prepare faucet transaction");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    wait_for_tx(&pending_tx).await;

    // -----------------------------------------------------------------------
    // Step 3: Call a template function (e.g. constructor / instantiate)
    //
    // StableCoin::for_template gives you the TemplateInterface, which only
    // exposes functions without self (e.g. constructors).
    // -----------------------------------------------------------------------

    let view_key = ootle_rs::template_types::crypto::RistrettoPublicKeyBytes::default();
    let tpl = StableCoin::for_template(stable_coin_template.as_template_address(), &provider);
    println!("Stable coin template: {}", tpl.template_address());

    let unsigned_tx = tpl
        .instantiate(view_key)
        .pay_fee(1000u64)
        .prepare()
        .await
        .expect("Failed to prepare instantiate");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    wait_for_tx(&pending_tx).await;
    // The component address would be extracted from the transaction receipt events/diff.

    // -----------------------------------------------------------------------
    // Step 4: Call component methods on an instantiated component
    //
    // StableCoin::for_component gives you the ComponentInterface, which only
    // exposes methods with &self / &mut self.
    // -----------------------------------------------------------------------

    let coin = StableCoin::for_component(stable_coin_component, &provider);
    println!("Stable coin component: {}", coin.component_address());

    // -- Example: Increase supply --
    // The macro generates a typed `increase_supply(Amount)` method.
    // Under the hood it calls `ComponentInvokeBuilder::call_method(component, "increase_supply", args![amount])`
    // and automatically discovers all vaults in the component for input resolution.
    let unsigned_tx = coin
        .increase_supply(Amount::new(1_000_000))
        .pay_fee(1000u64)
        .prepare()
        .await
        .expect("Failed to prepare increase_supply");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    wait_for_tx(&pending_tx).await;

    // -- Example: Set fee configuration --
    let coin = StableCoin::for_component(stable_coin_component, &provider);
    let unsigned_tx = coin
        .set_config_transfer_fee_percentage(2u8)
        .pay_fee(1000u64)
        .prepare()
        .await
        .expect("Failed to prepare set_config");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    wait_for_tx(&pending_tx).await;

    // -- Example: Using the generic IComponent builder directly --
    // For one-off calls where defining a full interface isn't worth it,
    // you can use IComponent directly with string method names.
    let unsigned_tx = IComponent::new(&provider)
        .call_method(stable_coin_component, "decrease_supply", tari_ootle_transaction::args![
            Amount::new(500_000)
        ])
        .pay_fee(1000u64)
        .prepare()
        .await
        .expect("Failed to prepare decrease_supply");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    wait_for_tx(&pending_tx).await;

    // -- Example: Chaining with workspace piping --
    // Withdraw returns a Bucket that can be piped to another method.
    let coin = StableCoin::for_component(stable_coin_component, &provider);
    let unsigned_tx = coin
        .withdraw(Amount::new(100_000))
        .put_last_instruction_output_on_workspace("bucket")
        // workspace!() returns a NamedArg which implements IntoArg,
        // so it can be passed directly to typed methods
        .deposit(tari_ootle_transaction::workspace!("bucket"))
        // Use .then() as an escape hatch to the raw TransactionBuilder
        .then(|b| {
            b.call_method(
                stable_coin_component,
                "increase_supply",
                tari_ootle_transaction::args![42u64],
            )
        })
        .pay_fee(1000u64)
        .prepare()
        .await
        .expect("Failed to prepare withdraw + deposit");

    let transaction = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(provider.wallet())
        .await
        .unwrap();
    let pending_tx = provider.send_transaction(transaction).await.unwrap();
    wait_for_tx(&pending_tx).await;

    println!("All operations completed successfully!");
}

async fn wait_for_tx(pending_tx: &PendingTransaction) {
    println!("Pending transaction... {}", pending_tx.tx_id());
    let outcome = pending_tx.watch().await.unwrap();
    println!("Transaction finalized: {:?}", outcome);

    let receipt = pending_tx.get_receipt().await.unwrap();
    println!("  Fees paid: {}", receipt.fee_receipt.total_fees_paid());
    if !receipt.events.is_empty() {
        println!("  Events:");
        for event in &*receipt.events {
            println!(
                "    {} [{}] {}",
                event.topic(),
                event.substate_id().display(),
                event.payload()
            );
        }
    }
    println!();
}
