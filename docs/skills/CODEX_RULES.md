# Tari Ootle — Codex Agent Rules

Use these instructions to generate accurate Tari Ootle templates and client code aligned with repository APIs.

## Essentials
- Templates: `tari_template_lib` (+ `prelude::*`)
- Types: `tari_template_lib_types`
- Transactions: `tari_ootle_transaction` (`TransactionBuilder`, `args!`)
- Client: `ootle-rs` (wallet, provider, faucet helper)
- Tests: `tari_template_test_tooling`

## Template Pattern
```rust
use tari_template_lib::prelude::*;

#[template]
mod app {
    use super::*;

    pub struct App { store: Vault }

    impl App {
        pub fn new() -> Component<Self> {
            let r = ResourceBuilder::public_fungible().with_token_symbol("CDX").build();
            Component::new(Self { store: Vault::new_empty(r) })
                .with_access_rules(ComponentAccessRules::new()
                    .method("deposit_all", rule!(allow_all))
                    .default(rule!(deny_all)))
                .create()
        }

        pub fn deposit_all(&mut self, mut b: Bucket) {
            self.store.deposit(b);
        }
    }
}
```

## Correct APIs
- Rules: `rule!(allow_all|deny_all|resource(addr)|any_of(...)|all_of(...)|m_of_n(...))`
- Auth: `CallerContext::transaction_signer_public_key()`
- Cross-component: `ComponentManager::get(addr).call(...)` (returns value) or `.invoke(...)` (unit)
- Events: `emit_event("Topic", metadata!["k" => v.to_string()])`
- Vault ops: `Vault::new_empty(addr)`, `Vault::from_bucket(b)`, `deposit(b)`, `withdraw(amount)`, `withdraw_non_fungible(id)`, `balance()`
- NFT mint: `vault.get_resource_manager().mint_non_fungible(id, &metadata![...], &())`

## Client Snippets
```rust
let wallet = OotleWallet::from(PrivateKeyProvider::random(Network::Esmeralda));
let mut provider = ProviderBuilder::new().wallet(wallet).connect(default_indexer_url(Network::Esmeralda)).await?;

// Publish template
let wasm = std::fs::read("target/wasm32-unknown-unknown/release/app.wasm")?;
let unsigned = TransactionBuilder::new(provider.network()).publish_template(wasm.try_into().unwrap()).build_unsigned();
let tx = TransactionRequest::default().with_transaction(unsigned).build(provider.wallet()).await?;
let receipt = provider.send_transaction(tx).await?.watch().await?;
```

## Build & Test
```bash
cargo build --target wasm32-unknown-unknown --release
```
Use `TemplateTest` to publish and call templates locally.
