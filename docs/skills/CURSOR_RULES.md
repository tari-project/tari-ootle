# Tari Ootle Development Rules

You are an expert in building decentralized applications on the Tari Ootle platform. Templates are written in Rust, compiled to WASM (`wasm32-unknown-unknown`), and deployed to the Tari L2 network. Follow these rules exactly.

## Architecture

- **Template**: A Rust module with `#[template]` that defines logic + state. Deployed as WASM.
- **Component**: A live on-chain instance of a template. Holds persistent state.
- **Resource**: A native digital asset — fungible (like ERC-20) or non-fungible (like ERC-721). Created via `ResourceBuilder`.
- **Vault**: On-chain container for exactly one resource type. MUST be stored in a component before function returns.
- **Bucket**: Temporary resource container during a transaction. MUST be consumed (deposited, burned, or returned) before function returns.
- **Transaction**: Atomic set of instructions (CallFunction, CallMethod). Signed, sent, and finalized.

## Crates

| Crate | Purpose |
|-------|---------|
| `tari_template_lib` | Core template library (prelude, ResourceBuilder, Vault, Bucket, macros) |
| `ootle-rs` | Client wallet, provider, signing |
| `tari_ootle_transaction` | `TransactionBuilder`, `args!` macro |
| `tari_template_lib_types` | Shared types (Amount, ResourceAddress, ComponentAddress) |
| `tari_template_test_tooling` | Dev-dependency test harness |

## Template Rules

### Always start templates with:
```rust
use tari_template_lib::prelude::*;

#[template]
mod template_name {
    use super::*;
    // ...
}
```

### Constructors
- A function returning `Self` implicitly creates a component on-chain.
- A function returning `Component<Self>` gives explicit control over access rules, owner rule, and address allocation.
- Use `Component::new(Self { ... }).with_access_rules(...).create()` for the explicit form.

### State
- All struct fields must be serde-serializable. Types from `tari_template_lib` already are.
- Methods with `&mut self` can modify state. Methods with `&self` are read-only.
- Structs inside the `#[template]` module auto-derive serde traits.

### Error Handling
- Use `panic!()` or `assert!()` to abort transactions. There are no Result-based error flows.
- On panic, the transaction fails atomically — no state changes are committed.

### Resources
```rust
// Fungible token
ResourceBuilder::public_fungible()
    .with_token_symbol("TOK")
    .metadata("name", "My Token")
    .initial_supply(Amount::from(1000))  // Returns Bucket
    .build();                             // Without initial_supply: returns ResourceAddress

// Non-fungible (NFT)
ResourceBuilder::non_fungible()
    .with_token_symbol("NFT")
    .build();  // Returns ResourceAddress; mint later via ResourceManager
```

### Minting NFTs
```rust
let manager = vault.get_resource_manager();
let bucket = manager.mint_non_fungible(
    nft_id,                                    // NonFungibleId
    &metadata!["key" => "value"],              // Immutable data
    &(),                                       // Mutable data
);
```

### Vault Operations
```rust
Vault::new_empty(resource_address)       // Empty vault for a resource
Vault::from_bucket(bucket)               // Vault from existing bucket
vault.deposit(bucket)                    // Add tokens
vault.withdraw(amount)                   // Withdraw fungible
vault.withdraw_non_fungible(nft_id)      // Withdraw specific NFT
vault.balance()                          // Check balance
vault.get_resource_manager()             // Get manager for minting
```

### Access Rules
```rust
ComponentAccessRules::new()
    .method("public_fn", rule!(allow_all))
    .method("admin_fn", rule!(resource(badge_resource)))
    .default(rule!(deny_all))
```

Rules: `allow_all`, `deny_all`, `require(resource)`, `require(any_of(a, b))`, `require(all_of(a, b))`

### Authentication
- ALWAYS use `CallerContext::transaction_signer_public_key()` to identify callers.
- NEVER accept a public key as a method argument for authentication — it can be spoofed.

### Cross-Component Calls
```rust
let other = ComponentManager::get(address);
let result: u64 = other.call("method_name", args![arg1, arg2]);
other.invoke("method_name", args![arg1, arg2]);
```

### Events
```rust
emit_event("EventName", metadata!["field" => value.to_string()]);
```

### Randomness
```rust
use tari_template_lib::rand::random_bytes;
let num = random_bytes(1)[0] % 11;
```
WARNING: Deterministic, not cryptographically secure.

## Compilation
```bash
cargo build --target wasm32-unknown-unknown --release
```
Output: `target/wasm32-unknown-unknown/release/<name>.wasm`

## Client-Side Interaction (ootle-rs)

### Provider Setup
```rust
use ootle_rs::{key_provider::PrivateKeyProvider, provider::ProviderBuilder, wallet::OotleWallet, default_indexer_url};
use tari_ootle_common_types::Network;

let secret = PrivateKeyProvider::random(Network::Esmeralda);
let wallet = OotleWallet::from(secret);
let mut provider = ProviderBuilder::new()
    .wallet(wallet)
    .connect(default_indexer_url(Network::Esmeralda))
    .await?;
```

### Testnet Faucet
```rust
use ootle_rs::{TransactionRequest, builtin_templates::{UnsignedTransactionBuilder, faucet::IFaucet}};
use tari_template_lib_types::constants::ONE_XTR;

let unsigned_tx = IFaucet::new(&provider).take_faucet_funds(10 * ONE_XTR).pay_fee(500u64).prepare().await?;
let tx = TransactionRequest::default().with_transaction(unsigned_tx).build(provider.wallet()).await?;
provider.send_transaction(tx).await?.watch().await?;
```

### Transaction Pattern
```rust
// 1. Build unsigned transaction
let unsigned_tx = TransactionBuilder::new(provider.network())
    .with_auto_fill_inputs()
    .pay_fee_from_component(account_addr, 2000u64)
    .call_function(template_addr, "new", args![])     // OR .call_method(component_addr, "method", args![...])
    .build_unsigned();

// 2. Sign
let tx = TransactionRequest::default().with_transaction(unsigned_tx).build(provider.wallet()).await?;

// 3. Send and watch
let receipt = provider.send_transaction(tx).await?.watch().await?;
```

### Reading Receipts
```rust
// Component address from creation
let addr = receipt.diff_summary.upped.iter()
    .find_map(|s| s.substate_id.as_component_address()).unwrap();

// Resource address (excluding native XTR)
let res = receipt.diff_summary.upped.iter()
    .find_map(|s| s.substate_id.as_resource_address().filter(|a| *a != XTR)).unwrap();

// Events
let event = receipt.events.iter().find(|e| e.topic() == "Template.Event").unwrap();
let val = event.get_payload("field");
```

### Manual Inputs
When auto-fill can't detect all substates:
```rust
builder.add_input(substate_address)
    .with_inputs(addrs.iter().copied().map(Into::into))
```

## Example: Guessing Game Template

```rust
use tari_template_lib::prelude::*;

#[template]
mod guessing_game {
    use super::*;

    pub struct Guess {
        pub payout_to: ComponentManager,
        pub guess: u8,
    }

    pub struct GuessingGame {
        prize_vault: Vault,
        guesses: HashMap<RistrettoPublicKeyBytes, Guess>,
        round_number: u32,
    }

    impl GuessingGame {
        pub fn new() -> Component<Self> {
            let prize_resource = ResourceBuilder::non_fungible()
                .metadata("name", "Guessing Game Prize")
                .with_token_symbol("🎲")
                .build();

            Component::new(Self {
                prize_vault: Vault::new_empty(prize_resource),
                guesses: HashMap::new(),
                round_number: 0,
            })
            .with_access_rules(ComponentAccessRules::new()
                .method("guess", rule![allow_all]))
            .create()
        }

        pub fn start_game(&mut self, prize: NonFungibleId) {
            self.round_number += 1;
            let manager = self.prize_vault.get_resource_manager();
            let prize = manager.mint_non_fungible(
                prize, &metadata!["round" => self.round_number.to_string()], &(),
            );
            self.prize_vault.deposit(prize);
        }

        pub fn guess(&mut self, guess: u8, payout_to: ComponentAddress) {
            let player = CallerContext::transaction_signer_public_key();
            let payout_to = ComponentManager::get(payout_to);
            let prev = self.guesses.insert(player, Guess { payout_to, guess });
            assert!(prev.is_none(), "You already guessed in this round");
        }

        pub fn end_game_and_payout(&mut self) {
            let mut prize = self.prize_vault.withdraw(1u64);
            let number = generate_number();
            let guesses = std::mem::take(&mut self.guesses);
            for (_player, guess) in guesses {
                if guess.guess == number {
                    guess.payout_to.invoke("deposit", args![prize]);
                    return;
                }
            }
            prize.burn();
        }
    }

    fn generate_number() -> u8 {
        use tari_template_lib::rand::random_bytes;
        random_bytes(1)[0] % 11
    }
}
```

## Testing
Use `tari_template_test_tooling` as a dev-dependency. Run `cargo test` to execute template tests against the local WASM execution engine.

## Common Mistakes to Avoid
1. Forgetting to store a Vault in a component — transaction will fail.
2. Not consuming a Bucket (deposit, burn, or return it) — transaction will fail.
3. Accepting a public key as a function argument for auth — use CallerContext instead.
4. Using `rand` crate directly — use `tari_template_lib::rand::random_bytes`.
5. Forgetting `.with_access_rules()` — by default only the component creator can call methods.
6. Depositing the wrong resource type into a vault — transaction will fail.
