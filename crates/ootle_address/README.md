# tari_ootle_address

[![Crates.io](https://img.shields.io/crates/v/tari_ootle_address.svg)](https://crates.io/crates/tari_ootle_address)
[![Documentation](https://docs.rs/tari_ootle_address/badge.svg)](https://docs.rs/tari_ootle_address)

Bech32m-encoded user-facing addresses for the Tari Ootle network. An
`OotleAddress` encodes an account public key, a view-only key, the target
network, and an optional opaque payment reference (`PayRef`) into a single
human-readable string.

## Format

```
otl_<account_pub_key><view_only_pub_key>[pay_ref]<checksum>
```

The human-readable part (HRP) encodes the network:

| Network   | HRP        |
|-----------|------------|
| MainNet   | `otl_`     |
| Esmeralda | `otl_esm_` |
| NextNet   | `otl_nxt_` |
| StageNet  | `otl_stg_` |
| LocalNet  | `otl_loc_` |

## Example

```rust
use tari_ootle_address::{OotleAddress, PayRef, Network};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

// Construct from raw key bytes
let account_key = RistrettoPublicKeyBytes::from([1u8; 32]);
let view_only_key = RistrettoPublicKeyBytes::from([2u8; 32]);

let addr = OotleAddress::new(Network::Esmeralda, view_only_key, account_key);
// Encodes to a bech32m string: "otl_1..."
let encoded = addr.to_string();

// Round-trip via FromStr / Display
let decoded: OotleAddress = encoded.parse().unwrap();
assert_eq!(addr, decoded);

// Optionally attach a payment reference (up to 64 bytes)
// When a user pays to this address, the reference is automatically included in the UTXO encrypted data when performing stealth payments
let pay_ref = PayRef::new_checked(b"invoice-42".to_vec()).unwrap();
let addr_with_ref = addr.with_pay_ref(pay_ref);
```

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
