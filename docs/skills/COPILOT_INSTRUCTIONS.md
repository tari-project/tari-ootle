# Tari Ootle Development Instructions for GitHub Copilot

## Context

You are assisting with development on the **Tari Ootle** platform — a decentralized application layer built on Tari L2. Templates (smart contracts) are written in Rust, compiled to WASM (`wasm32-unknown-unknown`), and deployed to the network.

## Core Concepts

| Concept | Description |
|---------|-------------|
| Template | Rust module with `#[template]` macro. Compiled to WASM. Defines logic and state structure. |
| Component | On-chain instance of a template. Holds persistent state. |
| Resource | Native digital asset — public fungible, public non-fungible, confidential, or stealth. |
| Vault | On-chain container for exactly one resource type. Must be stored in a component. |
| Bucket | Temporary resource container during transactions. Must be consumed before function returns. |
| Transaction | Atomic instruction set (CallFunction, CallMethod). Signed and submitted via provider. |

## Required Crates

- `tari_template_lib` — Template development (prelude, ResourceBuilder, Vault, Bucket, access rules)
- `ootle-rs` — Client-side wallet, provider, signing
- `tari_ootle_transaction` — TransactionBuilder, `args!` macro
- `tari_template_lib_types` — Shared types (Amount, ResourceAddress, ComponentAddress)
- `tari_template_test_tooling` — Test harness (dev-dependency only)

## Template Skeleton

```rust
use tari_template_lib::prelude::*;

#[template]
mod my_app {
    use super::*;

    pub struct MyApp {
        vault: Vault,
        state_field: u64,
    }

    impl MyApp {
        // Constructor — returns Component<Self> for access rule control
        pub fn new() -> Component<Self> {
            let resource = ResourceBuilder::public_fungible()
                .with_token_symbol("SYM")
                .build();

            Component::new(Self {
                vault: Vault::new_empty(resource),
                state_field: 0,
            })
            .with_access_rules(ComponentAccessRules::new()
                .method("public_method", rule!(allow_all))
                .default(rule!(deny_all))
            )
            .create()
        }

        pub fn public_method(&mut self) { /* ... */ }
    }
}
```

## Key Patterns

### Creating Resources
```rust
// Fungible
ResourceBuilder::public_fungible().with_token_symbol("TOK").build();
// With initial supply (returns Bucket)
ResourceBuilder::public_fungible().with_token_symbol("TOK").initial_supply(Amount::from(100)).build();
// Non-fungible
ResourceBuilder::non_fungible().with_token_symbol("NFT").build();
```

### Minting NFTs
```rust
let manager = vault.get_resource_manager();
let bucket = manager.mint_non_fungible(id, &metadata!["key" => "val"], &());
vault.deposit(bucket);
```

### Vault/Bucket Operations
```rust
vault.deposit(bucket);                       // Add to vault
let b = vault.withdraw(amount);              // Remove from vault (fungible)
let b = vault.withdraw_non_fungible(id);     // Remove specific NFT
vault.balance();                             // Check balance
bucket.burn();                               // Destroy tokens permanently
```

### Access Rules
```rust
ComponentAccessRules::new()
    .method("open_method", rule!(allow_all))
    .method("restricted", rule!(resource(badge)))
    .default(rule!(deny_all))
```

### Authentication
```rust
// ALWAYS use this — cannot be spoofed
let signer = CallerContext::transaction_signer_public_key();
// NEVER accept public key as argument for auth purposes
```

### Cross-Component Calls
```rust
let comp = ComponentManager::get(address);
let n: u64 = comp.call("method", args![arg1, arg2]);
comp.invoke("method", args![arg1, arg2]);
```

### Events
```rust
emit_event("EventName", metadata!["field" => value.to_string()]);
```

### Randomness
```rust
use tari_template_lib::rand::random_bytes;
let n = random_bytes(1)[0] % 11;  // 0..=10, deterministic, NOT cryptographically secure
```

### Error Handling
```rust
assert!(condition, "error message");
panic!("something went wrong");
// Panics abort the transaction atomically
```

## Client-Side (ootle-rs)

### Setup
```rust
let secret = PrivateKeyProvider::random(Network::Esmeralda);
let wallet = OotleWallet::from(secret);
let mut provider = ProviderBuilder::new()
    .wallet(wallet)
    .connect(default_indexer_url(Network::Esmeralda))
    .await?;
```

### Transaction Flow
```rust
// Build
let unsigned_tx = TransactionBuilder::new(provider.network())
    .with_auto_fill_inputs()
    .pay_fee_from_component(account, 2000u64)
    .call_function(template, "new", args![])
    .build_unsigned();
// Sign
let tx = TransactionRequest::default().with_transaction(unsigned_tx).build(provider.wallet()).await?;
// Send + Wait
let receipt = provider.send_transaction(tx).await?.watch().await?;
```

### Faucet (Testnet)
```rust
let unsigned_tx = IFaucet::new(&provider).take_faucet_funds(10 * ONE_XTR).pay_fee(500u64).prepare().await?;
```

### Reading Receipts
```rust
let component = receipt.diff_summary.upped.iter()
    .find_map(|s| s.substate_id.as_component_address());
let event = receipt.events.iter().find(|e| e.topic() == "App.Event");
```

## Compilation
```bash
cargo build --target wasm32-unknown-unknown --release
```

## Critical Rules
1. **Vaults must be stored in components** — orphaned vaults fail the transaction.
2. **Buckets must be consumed** — deposit, burn, or return before function ends.
3. **Use CallerContext for auth** — never accept public keys as arguments for identity.
4. **Use `tari_template_lib::rand`** — not the `rand` crate (no entropy on wasm32-unknown-unknown).
5. **Panics = transaction failure** — this is the error handling mechanism.
6. **One resource per vault** — depositing wrong type fails.
7. **Structs in `#[template]` modules** get automatic serde derives.
