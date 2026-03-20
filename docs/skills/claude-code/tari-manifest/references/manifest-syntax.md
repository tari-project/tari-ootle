# Tari Ootle Manifest Syntax Reference

Complete reference for the Tari transaction manifest DSL. The manifest parser lives at `crates/transaction_manifest/`.

## File Structure

A manifest file contains:
1. **Template imports** (optional) - `use template_<hex> as Alias;` or `use PredefinedTemplate;`
2. **`fn fee_main() { ... }`** (optional) - Fee payment instructions, executed before main
3. **`fn main() { ... }`** (required) - Main transaction instructions
4. **Helper functions** (optional) - `fn helper() { ... }`, inlined at call sites (max depth 16)

The `Account` template is always pre-imported automatically.

## Template Imports

### By hash (64-character hex)

```rust
use template_687b0d5b3bee2e987a72c0f8b0b9286968803eba9040ed67e3a85b8465ad294a as TestFaucet;
```

### Predefined templates

```rust
use Account;  // Already pre-imported, but can be explicit
```

### Via external template map

Templates can also be provided programmatically via the `templates: HashMap<String, TemplateAddress>` parameter to `parse_manifest()`, avoiding the need for inline `use` statements. The test tooling (`TemplateTest`) does this automatically for all registered templates.

## Statements

### Variable Assignment from Globals

Three equivalent macros for accessing runtime variables:

```rust
let my_var = var!["name"];      // Most common
let my_var = arg!["name"];      // Alias
let my_var = global!["name"];   // Alias
```

The string inside the macro is the key used to look up the value in the `globals` HashMap passed to `parse_manifest()`.

### Template Function Calls

Static function calls on imported templates:

```rust
// With return value
let component = MyTemplate::new(arg1, arg2);

// Without return value (fire-and-forget)
MyTemplate::do_something(arg1);
```

### Component Method Calls

Method calls on variables (components or workspace results):

```rust
// With return value
let result = component.method(arg1, arg2);

// Without return value
component.method(arg1);
```

### Address Allocation

Pre-allocate addresses for components or resources before they are created:

```rust
let addr = new_component_addr!();
let addr = new_resource_addr!();
let addr = allocate_component_address!();  // Alias
```

### Logging

```rust
info!("Informational message");
debug!("Debug message");
warn!("Warning message");
error!("Error message");
```

Note: only string literals are supported (no format args).

### Proof Management

```rust
drop_all_proofs!();
```

Drops all proofs currently in the workspace. Typically called after protected operations.

### Local Function Calls

Define helper functions and call them from `main()` or `fee_main()`:

```rust
fn setup() {
    let faucet = var!["faucet"];
    let coins = faucet.take_free_coins();
    // ...
}

fn main() {
    setup();
    // ...
}
```

Helper functions are inlined at call sites. Maximum nesting depth is 16.

## Argument Types - Detailed Reference

### Integer Literals

Unsuffixed integers default to `i128`. Always use a suffix for explicit typing:

| Suffix | Rust type | Example |
|--------|-----------|---------|
| `u8` | `u8` | `255u8` |
| `u16` | `u16` | `1000u16` |
| `u32` | `u32` | `42u32` |
| `u64` | `u64` | `1_000_000u64` |
| `u128` | `u128` | `100u128` |
| `i8` | `i8` | `-1i8` |
| `i16` | `i16` | `500i16` |
| `i32` | `i32` | `42i32` |
| `i64` | `i64` | `100i64` |
| (none) or `i128` | `i128` | `100` or `100i128` |

Underscores are allowed as visual separators: `1_000_000u64`.

**Float literals are NOT supported.**

### String Literals

Standard Rust string literals:

```rust
"hello world"
```

### Boolean Literals

```rust
true
false
```

### Byte String Literals

```rust
b"raw bytes"
```

### Amount

Wraps an integer as a `tari_template_lib::types::Amount`:

```rust
Amount(1000)
```

### Address and SubstateId

Reference a substate by its address string. Both accept either a string literal or a workspace variable:

```rust
// String literal form
Address("component_0123456789abcdef...")
SubstateId("vault_0123456789abcdef...")

// Variable form (from a prior let binding)
Address(my_var)
SubstateId(my_var)
```

Address prefixes: `component_`, `resource_`, `vault_`, `nft_`, `txreceipt_`, `template_`, `validatorfeeclaim_`, `utxo_`, `tombstone_`.

### NonFungibleId

Three forms:

```rust
NonFungibleId("StringId")      // String-based NFT ID
NonFungibleId(1u32)            // 32-bit integer ID
NonFungibleId(42u64)           // 64-bit integer ID
// Also: byte string for 256-bit ID
```

A suffix is required for integer forms.

### Metadata

Key-value metadata string:

```rust
Metadata("key=value")
```

Parsed as `tari_template_lib::types::Metadata`.

### PublicKey and HexBytes

Both parse a hex string into raw bytes:

```rust
PublicKey("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
HexBytes("deadbeef")
```

### CBOR Values

Two forms for embedding arbitrary CBOR-encoded data:

```rust
// Function form: JSON string parsed to CBOR
Cbor("{\"key\": \"value\"}")

// Macro form: JSON literal parsed to CBOR
cbor!({"key": {"nested": [1, 2, 3]}})
```

### TARI

Constant representing the native Tari token resource address. Use `TARI` in all new manifests. `XTR` is a deprecated alias that still works but should not be used.

```rust
let bucket = account.withdraw(TARI, 1000);
```

### Workspace Variables

Bare identifiers reference results from prior instructions:

```rust
let bucket = account.withdraw(TARI, 100);
shop.buy(bucket);  // 'bucket' references the withdraw result
```

## ManifestValue (Rust API)

When passing globals to `parse_manifest()`, construct `ManifestValue` instances:

```rust
use tari_transaction_manifest::ManifestValue;
use tari_engine_types::substate::SubstateId;

// From component/resource/vault addresses (anything Into<SubstateId>)
let val = ManifestValue::from(component_address);  // ComponentAddress
let val = ManifestValue::from(resource_address);    // ResourceAddress

// From arbitrary serializable data
let val = ManifestValue::new_value(&my_struct)?;

// Parse from string (tries SubstateId, then NonFungibleId, then literal)
let val: ManifestValue = "component_ab12...".parse()?;
let val: ManifestValue = "1000u64".parse()?;
```

## Execution Model

1. Fee instructions from `fee_main()` execute first
2. If fees are accepted, main instructions from `main()` execute
3. Each instruction that produces output stores it in a workspace slot (auto-assigned sequential IDs)
4. Workspace variables in subsequent instructions reference these slots
5. The workspace is shared across all instructions in the transaction

## Error Types

The parser produces `ManifestError` variants:
- `LexError` - tokenization failure
- `SyntaxError` - Rust parsing error
- `TemplateNotImported` - using an unregistered template alias
- `UndefinedGlobal` - referencing a variable not in globals map
- `UndefinedVariable` - referencing a workspace variable that doesn't exist
- `UndefinedFunction` - calling a helper function that doesn't exist
- `MaxCallDepthExceeded` - helper function nesting exceeds 16
- `InvalidVariableType` - type mismatch in variable usage

## Crate Location

The manifest parser is at `crates/transaction_manifest/`:
- `src/lib.rs` - `parse_manifest()` entry point
- `src/parser.rs` - Rust-syn based parser
- `src/generator.rs` - AST to instruction compiler
- `src/value.rs` - `ManifestValue` type and conversions
- `src/ast.rs` - AST definitions
- `src/error.rs` - Error types
