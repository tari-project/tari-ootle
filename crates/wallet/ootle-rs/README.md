# ootle-billon

[![Crates.io](https://img.shields.io/crates/v/ootle-billon.svg)](https://crates.io/crates/ootle-billon)
[![Documentation](https://docs.rs/ootle-billon/badge.svg)](https://docs.rs/ootle-billon)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-APACHE)

**High-performance, familiar interaction with the Tari Ootle (Layer 2) network.**

`ootle-billon` is a pure Rust library designed to be the standard interface for interacting with Tari Ootle. It is
architected to mirror the interface of [alloy-rs](https://github.com/alloy-rs/alloy), providing a seamless developer
experience for those transitioning from Ethereum or generic blockchain development to the Tari ecosystem.

## 🪙 Why "Billon"?

**Billon** is an ancient alloy historically used for everyday transactions. The metaphor:
if Minotari (L1) is digital gold, Ootle (L2) is the **Billon**—built for speed and utility. The name also pays homage to
the `alloy-rs` library which this library emulates.

## ✨ Features

* **Alloy-like API:** Uses the familiar `Provider`, `Signer`, and `Transport` architecture found in `alloy`.
* **Ootle Native:** First-class support for Tari Ootle
* **Type-Safe:** Strongly typed interactions with Tari's confidential assets.
* **Async/Await:** Built on `tokio` for high-performance, non-blocking I/O.

## 📦 Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
ootle-billon = "0.1.0"
```

## 🚀 Quick Start

Connect to a local Ootle indexer, create a wallet, and send a transaction.

```rust
use ootle_billon::prelude::*;
use ootle_billon::providers::ProviderBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup a signer (Burner wallet or standard key)
    let signer = PrivateKeySigner::random();
    let address = signer.address();
    let wallet = OotleWallet::new(signer);
    let indexer_api_url = "http://localhost:12000";

    // 2. Create a provider with the Ootle network configuration
    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_ootle_testnet()
        .connect(indexer_api_url)
        .await?;

    println!("🤖 Ootle Agent active at: {}", address);

    // 3. Craft a transaction (e.g., calling a template method)
    let tx_id = provider
        .send_transaction(
            TransactionRequest::default()
                .to(some_recipient_address)
                .with_amount(100_u64)
                .with_template_call("transfer", args![])
        )
        .await?;
    provider.get_transaction_receipt(tx_id)
        .await?;

    println!("✅ Transaction confirmed: {:?}", tx_hash);

    Ok(())
}
```

## 🏗 Architecture

ootle-billon follows the modular design of alloy:

- Core: Defines the primitive types (Addresses, Signatures, Confidential Commitments) specific to Tari.
- Transport: Handles the JSON-RPC (or gRPC) communication with Ootle Validator Nodes (VNs).
- Provider: The high-level API for sending requests and managing state.
- Signer: Abstractions for signing transactions (support for Ristretto keys).

## 🤝 Contributing

We welcome contributions! Please see CONTRIBUTING.md for details on how to get started.

## 📄 License

This project is dual-licensed under either:
Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)
at your option.

---

"To the Ootle!" 🦉