# Tari Ootle Conventions for Aider

## What is Tari Ootle?

A decentralized application platform on Tari L2. Smart contracts ("templates") are written in Rust, compiled to WASM (`wasm32-unknown-unknown`), and deployed to the network as components with persistent on-chain state.

## Key Types

- `ResourceAddress` — Unique identifier for a resource (token/NFT type)
- `ComponentAddress` — Unique identifier for a deployed component instance
- `TemplateAddress` — Unique identifier for a deployed template
- `Vault` — On-chain resource container (one resource type only)
- `Bucket` — Temporary resource container during execution
- `Amount` — Numeric amount type for fungible resources
- `NonFungibleId` — Unique identifier for an NFT within a resource
- `ComponentManager` — Wrapper for cross-component calls
- `RistrettoPublicKeyBytes` — Public key type for signer identification

## Template Conventions

### File Structure
```
my_template/
├── Cargo.toml
├── src/lib.rs       # Template code
└── tests/test.rs    # Tests using tari_template_test_tooling
```

### Scaffold with cargo-generate
```bash
cargo generate https://github.com/tari-project/wasm-template
```

### Standard Template Layout
```rust
use tari_template_lib::prelude::*;

#[template]
mod template_name {
    use super::*;

    // Structs inside #[template] auto-derive serde
    pub struct MyData {
        pub field: String,
    }

    pub struct MyComponent {
        vault: Vault,
        data: HashMap<RistrettoPublicKeyBytes, MyData>,
        counter: u32,
    }

    impl MyComponent {
        // Constructor
        pub fn new() -> Component<Self> {
            let resource = ResourceBuilder::non_fungible()
                .with_token_symbol("SYM")
                .metadata("name", "My Resource")
                .build();

            Component::new(Self {
                vault: Vault::new_empty(resource),
                data: HashMap::new(),
                counter: 0,
            })
            .with_access_rules(ComponentAccessRules::new()
                .method("public_method", rule!(allow_all))
            )
            .create()
        }

        // Public method (anyone can call)
        pub fn public_method(&mut self, val: u8, target: ComponentAddress) {
            let signer = CallerContext::transaction_signer_public_key();
            assert!(!self.data.contains_key(&signer), "Already participated");
            self.data.insert(signer, MyData { field: val.to_string() });
        }

        // Owner-only method (default access rule)
        pub fn owner_action(&mut self) {
            self.counter += 1;
            let mgr = self.vault.get_resource_manager();
            let nft = mgr.mint_non_fungible(
                NonFungibleId::from_string(&format!("item-{}", self.counter)),
                &metadata!["counter" => self.counter.to_string()],
                &(),
            );
            self.vault.deposit(nft);
        }

        // Payout with cross-component call
        pub fn payout(&mut self, recipient: ComponentAddress) {
            let bucket = self.vault.withdraw(1u64);
            let comp = ComponentManager::get(recipient);
            comp.invoke("deposit", args![bucket]);
        }
    }

    // Helper functions (not exposed as methods)
    fn helper() -> u8 {
        use tari_template_lib::rand::random_bytes;
        random_bytes(1)[0] % 10
    }
}
```

## Resource Types

| Type | Builder | Use Case |
|------|---------|----------|
| Public Fungible | `ResourceBuilder::public_fungible()` | Tokens, currencies |
| Public Non-Fungible | `ResourceBuilder::non_fungible()` | NFTs, unique items |
| Confidential | Fungible with hidden amounts | Privacy tokens |
| Stealth | Confidential UTXOs | XTR native token |

## Client-Side Interaction

### Provider Setup
```rust
let wallet = OotleWallet::from(PrivateKeyProvider::random(Network::Esmeralda));
let mut provider = ProviderBuilder::new()
    .wallet(wallet)
    .connect(default_indexer_url(Network::Esmeralda))
    .await?;
```

### Transaction Flow
1. Build: `TransactionBuilder::new(network).with_auto_fill_inputs().pay_fee_from_component(acct, fee).call_function/call_method(...).build_unsigned()`
2. Sign: `TransactionRequest::default().with_transaction(utx).build(wallet).await?`
3. Send: `provider.send_transaction(tx).await?.watch().await?`

### Receipt Inspection
```rust
// New component address
receipt.diff_summary.upped.iter().find_map(|s| s.substate_id.as_component_address())
// New resource address
receipt.diff_summary.upped.iter().find_map(|s| s.substate_id.as_resource_address().filter(|a| *a != XTR))
// Events
receipt.events.iter().find(|e| e.topic() == "Template.EventName")
```

## Invariants (MUST follow)

1. Every `Vault` must be stored in a component before function returns.
2. Every `Bucket` must be consumed (deposited/burned/returned) before function returns.
3. Authentication: use `CallerContext::transaction_signer_public_key()`, never accept pubkey args.
4. Randomness: use `tari_template_lib::rand::random_bytes`, not `rand` crate.
5. Errors: use `panic!`/`assert!` — panics abort transactions atomically.
6. One vault holds exactly one resource type.
7. Default access: only component creator can call methods unless overridden.
8. Compile target: `wasm32-unknown-unknown`.
