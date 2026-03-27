# `tari_bor`

## Overview

`tari_bor` is the low-level self-describing Binary Object Representation (BOR) used in Tari.

It provides a thin api over the [ciborium](https://crates.io/crates/ciborium) crate.

## Usage

### Example: Serializing an Object

```rust
use tari_bor::serialize;

fn main() {
    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    struct TestCase {
        bytes: Vec<u8>,
        pk: String,
    }

    let test_case = TestCase {
        bytes: vec![1, 2, 3, 4, 5],
        pk: RistrettoPublicKeyBytes::from([1; 32]),
    };
    let encoded = tari_bor::encode(&test_case).unwrap();
    let decoded: TestCase = tari_bor::decode(&encoded).unwrap();
}
```

## Documentation

Detailed documentation is available at [docs.rs/tari_bor](https://docs.rs/tari_bor).