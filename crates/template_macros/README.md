# tari_template_macros

[![Crates.io](https://img.shields.io/crates/v/tari_template_macros.svg)](https://crates.io/crates/tari_template_macros)
[![Documentation](https://docs.rs/tari_template_macros/badge.svg)](https://docs.rs/tari_template_macros)

Procedural macros for writing Tari smart-contract templates. The `#[template]`
attribute transforms an annotated module into a fully wired template: it
generates the ABI descriptor, the WASM-exported entry-point functions, and the
internal dispatcher that routes engine calls to the correct methods.

## Usage

Annotate a module that contains a struct and its `impl` block:

```rust
use tari_template_lib::prelude::*;

#[template]
mod counter {
    pub struct Counter {
        value: u64,
    }

    impl Counter {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn increment(&mut self) {
            self.value += 1;
        }

        pub fn get(&self) -> u64 {
            self.value
        }
    }
}
```

The macro expands this into:

- A `TemplateDefinition` describing every public method and its argument types.
- `#[no_mangle] extern "C"` ABI functions for each method, usable by the WASM host.
- A dispatcher that decodes engine call arguments and routes them.

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
