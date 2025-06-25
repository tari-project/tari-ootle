# `tari_template_lib`

## Overview

`tari_template_lib` provides ergonomic abstractions that allow WASM templates to interact with the Tari Ootle engine.
Most if not all Ootle templates written in rust should depend on this crate.

In most cases, you will only require the `prelude` which can be included with:

```
use tari_template_lib::prelude::*;
```

Typically, a template author will use structs exported from the [models] module, the
[ResourceBuilder](resource::ResourceBuilder) and the [ComponentBuilder](component::ComponentBuilder). This crate
re-exports low-level ABI functions in `tari_template_abi` and the `tari_template_macros` proc macro.

## Template Examples

- <https://github.com/tari-project/wasm-template>
- <https://github.com/tari-project/wasm-examples>
- <https://github.com/tari-project/tari-ootle/tree/development/crates/engine/tests/templates>

## no_std

no_std can be enabled using the `no_std` feature flag.

## Documentation

Detailed documentation is available at [docs.rs/tari_template_lib](https://docs.rs/tari_template_lib).