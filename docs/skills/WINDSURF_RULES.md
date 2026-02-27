# Tari Ootle — Windsurf AI Rules

## Platform Overview

Tari Ootle is a decentralized smart contract platform. You write **templates** in Rust, compile to WASM, and deploy to the Tari L2 network. Deployed instances are called **components** and hold on-chain state.

## Glossary

- **Template** — `#[template]`-annotated Rust module → compiled to WASM → deployed on-chain.
- **Component** — Instance of a template with persistent state on the blockchain.
- **Resource** — Native digital asset. Four types: public fungible, public non-fungible, confidential, stealth.
- **Vault** — On-chain container holding one resource type. Must live inside a component.
- **Bucket** — Temporary resource container during execution. Must be consumed.
- **args!** — Macro for serializing arguments to template/component calls.

## Template Pattern

```rust
use tari_template_lib::prelude::*;

#[template]
mod app {
    use super::*;

    pub struct App {
        vault: Vault,
        data: u64,
    }

    impl App {
        pub fn new() -> Component<Self> {
            let res = ResourceBuilder::public_fungible()
                .with_token_symbol("APP")
                .build();

            Component::new(Self {
                vault: Vault::new_empty(res),
                data: 0,
            })
            .with_access_rules(ComponentAccessRules::new()
                .method("open", rule!(allow_all))
                .default(rule!(deny_all)))
            .create()
        }

        pub fn open(&mut self) { self.data += 1; }
    }
}
```

## Resource Creation

```rust
// Fungible token
ResourceBuilder::public_fungible()
    .with_token_symbol("TOK")
    .metadata("name", "Token Name")
    .build();  // → ResourceAddress

// Fungible with initial supply
ResourceBuilder::public_fungible()
    .with_token_symbol("TOK")
    .initial_supply(Amount(1000))
    .build();  // → Bucket

// Non-fungible (NFT)
ResourceBuilder::non_fungible()
    .with_token_symbol("NFT")
    .metadata("name", "NFT Collection")
    .build();  // → ResourceAddress
```

## NFT Minting

```rust
let mgr = vault.get_resource_manager();
let bucket = mgr.mint_non_fungible(
    NonFungibleId::from_string("unique-id"),
    &metadata!["round" => "1"],  // immutable data
    &(),                          // mutable data
);
vault.deposit(bucket);
```

## Vault & Bucket

```rust
// Vault lifecycle
let v = Vault::new_empty(resource_addr);
let v = Vault::from_bucket(bucket);
v.deposit(bucket);
let b = v.withdraw(Amount(10));
let b = v.withdraw_non_fungible(nft_id);
v.balance();
v.get_resource_manager();

// Bucket must be consumed
bucket.burn();                              // destroy
vault.deposit(bucket);                      // store
other_component.invoke("deposit", args![bucket]); // transfer
```

## Access Rules

```rust
ComponentAccessRules::new()
    .method("public_fn", rule![allow_all])
    .method("admin_fn", rule![require(admin_resource)])
    .method("multi_auth", rule![require(any_of(a, b))])
    .default(rule![deny_all])
```

## Authentication

```rust
let signer = CallerContext::transaction_signer_public_key();
// NEVER take pubkey as argument — always use CallerContext
```

## Cross-Component Calls

```rust
let comp = ComponentManager::get(component_address);
comp.invoke("method_name", args![arg1, arg2]);
```

## Events & Randomness

```rust
emit_event("GameOver", metadata!["winner" => addr.to_string(), "round" => num.to_string()]);

use tari_template_lib::rand::random_bytes;
let n = random_bytes(1)[0] % 11;  // deterministic, NOT crypto-secure
```

## Error Handling

```rust
assert!(val > 0, "Must be positive");
panic!("Transaction aborted");
// Panics = atomic rollback, no state committed
```

## Compilation

```bash
cargo build --target wasm32-unknown-unknown --release
# Output: target/wasm32-unknown-unknown/release/<name>.wasm
```

## Client-Side (ootle-rs)

```rust
// Setup
let secret = PrivateKeyProvider::random(Network::Esmeralda);
let wallet = OotleWallet::from(secret);
let mut provider = ProviderBuilder::new()
    .wallet(wallet)
    .connect(default_indexer_url(Network::Esmeralda))
    .await?;

// Faucet
let utx = IFaucet::new(&provider).take_faucet_funds(10 * ONE_XTR).pay_fee(500u64).prepare().await?;
let tx = TransactionRequest::default().with_transaction(utx).build(provider.wallet()).await?;
provider.send_transaction(tx).await?.watch().await?;

// Call function (create component)
let utx = TransactionBuilder::new(provider.network())
    .with_auto_fill_inputs()
    .pay_fee_from_component(account, 2000u64)
    .call_function(template, "new", args![])
    .build_unsigned();

// Call method
let utx = TransactionBuilder::new(provider.network())
    .with_auto_fill_inputs()
    .pay_fee_from_component(account, 2000u64)
    .call_method(component, "method", args![val])
    .build_unsigned();

// Sign → Send → Watch
let tx = TransactionRequest::default().with_transaction(utx).build(provider.wallet()).await?;
let receipt = provider.send_transaction(tx).await?.watch().await?;

// Read results
let comp_addr = receipt.diff_summary.upped.iter()
    .find_map(|s| s.substate_id.as_component_address());
let event = receipt.events.iter().find(|e| e.topic() == "App.Event");
```

## Hard Rules

1. Vault MUST be stored in a component — orphaned = transaction failure.
2. Bucket MUST be consumed — orphaned = transaction failure.
3. Use `CallerContext::transaction_signer_public_key()` for auth — never accept pubkey args.
4. Use `tari_template_lib::rand::random_bytes` — not `rand` crate.
5. Panics abort transactions atomically.
6. One resource type per vault.
7. Structs inside `#[template]` module auto-derive serde.
8. By default, only component creator can call methods — set access rules for public access.
