# tari_engine

[![Crates.io](https://img.shields.io/crates/v/tari_engine.svg)](https://crates.io/crates/tari_engine)
[![Documentation](https://docs.rs/tari_engine/badge.svg)](https://docs.rs/tari_engine)

The Tari template execution engine. It loads compiled WASM templates, executes
transactions against them, manages state transitions, and enforces fee and
authorisation rules.

## Key components

| Component                           | Description                                                                 |
|-------------------------------------|-----------------------------------------------------------------------------|
| `TransactionProcessor`              | Drives execution of a `Transaction` through the runtime                     |
| `RuntimeModule`                     | Trait for plugging custom runtime behaviour (fee modules, call tracking, …) |
| `WasmModule` / `LoadedWasmTemplate` | Loads and instruments a compiled template binary with gas metering          |
| `MemoryStateStore`                  | In-process state backend used in tests and benchmarks                       |
| `FeeModule` / `FeeTable`            | Configurable fee schedule applied during execution                          |
| `AuthParams` / `AuthorizationScope` | Signer-key context propagated into every call frame                         |

> **Note:** The engine is typically consumed through `tari_template_test_tooling`
> in tests, or by validator nodes in production. Direct use is needed only when
> embedding the engine in a custom host.

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
