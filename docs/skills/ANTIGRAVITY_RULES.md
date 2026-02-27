# Tari Ootle — Antigravity Agent Rules

## Purpose
Antigravity should use these instructions to help developers create Tari Ootle templates (WASM smart contracts) and client transactions with accurate APIs and patterns from the codebase.

## Core References
- Templates and runtime: `tari_template_lib`, `tari_template_lib_types`
- Transactions: `tari_ootle_transaction`
- Client provider and wallet: `ootle-rs`
- Testing: `tari_template_test_tooling`

## Template Skeleton
```rust
use tari_template_lib::prelude::*;

#[template]
mod app {
    use super::*;

    pub struct App { vault: Vault, n: u64 }

    impl App {
        pub fn new() -> Component<Self> {
            let res = ResourceBuilder::public_fungible().with_token_symbol("APP").build();
            Component::new(Self { vault: Vault::new_empty(res), n: 0 })
                .with_access_rules(ComponentAccessRules::new()
                    .method("open", rule!(allow_all))
                    .default(rule!(deny_all)))
                .create()
        }

        pub fn open(&mut self) { self.n += 1; }
    }
}
```

## Rules and Patterns
- Access rules: `rule!(allow_all)`, `rule!(deny_all)`, `rule!(resource(addr))`, `rule!(any_of(...))`, `rule!(all_of(...))`, `rule!(m_of_n(...))`.
- Auth: `CallerContext::transaction_signer_public_key()` for identity. Do not accept public keys as arguments.
- Cross-component: `ComponentManager::get(addr).call("method", args![...])` or `.invoke(...)` for unit.
- Events: `emit_event("Topic", metadata!["k" => v.to_string()])`.
- Vault/Bucket: store `Vault` in component; consume `Bucket` (deposit/burn/return) before return.
- Randomness: `tari_template_lib::rand::random_bytes` (deterministic).

## Client Quickstart (ootle-rs)
```rust
use ootle_rs::{key_provider::PrivateKeyProvider, provider::ProviderBuilder, wallet::OotleWallet, default_indexer_url, TransactionRequest, builtin_templates::{UnsignedTransactionBuilder, faucet::IFaucet}};
use tari_ootle_common_types::Network;
use tari_ootle_transaction::{TransactionBuilder, args};
use tari_template_lib_types::constants::ONE_XTR;

let wallet = OotleWallet::from(PrivateKeyProvider::random(Network::Esmeralda));
let mut provider = ProviderBuilder::new().wallet(wallet).connect(default_indexer_url(Network::Esmeralda)).await?;
let unsigned = IFaucet::new(&provider).take_faucet_funds(10 * ONE_XTR).pay_fee(500u64).prepare().await?;
let tx = TransactionRequest::default().with_transaction(unsigned).build(provider.wallet()).await?;
let receipt = provider.send_transaction(tx).await?.watch().await?;
```

## Build
```bash
cargo build --target wasm32-unknown-unknown --release
```

## Testing
Use `tari_template_test_tooling::TemplateTest` to publish and call your template in-process.
