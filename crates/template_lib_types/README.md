# `tari_template_lib_types`

[![Crates.io](https://img.shields.io/crates/v/tari_template_lib_types.svg)](https://crates.io/crates/tari_template_lib_types)
[![Documentation](https://docs.rs/tari_template_lib_types/badge.svg)](https://docs.rs/tari_template_lib_types)

Primitive types shared between `tari_template_lib` and the host engine. Kept as
a separate, `no_std`-compatible crate so that both the WASM template environment
and native tooling can depend on the same definitions without pulling in the full
template library.
