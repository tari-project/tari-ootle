# tari_engine_types

[![Crates.io](https://img.shields.io/crates/v/tari_engine_types.svg)](https://crates.io/crates/tari_engine_types)
[![Documentation](https://docs.rs/tari_engine_types/badge.svg)](https://docs.rs/tari_engine_types)

Shared data types for the Tari template engine. This crate defines the core domain
model — substates, vaults, buckets, resources, proofs, transaction receipts, and
supporting cryptographic helpers — used across the engine, validator nodes, and
test tooling.

## Key types

| Type                      | Description                                                                            |
|---------------------------|----------------------------------------------------------------------------------------|
| `Substate` / `SubstateId` | Versioned on-chain state and its identifier                                            |
| `ComponentHeader`         | Metadata attached to a deployed component (template address, access rules, owner rule) |
| `Resource`                | Fungible or non-fungible resource definition                                           |
| `Vault`                   | On-chain container that holds a resource                                               |
| `Bucket`                  | Ephemeral resource container used during transaction execution                         |
| `Proof`                   | Proof used for authorisation checks                                                    |
| `TransactionReceipt`      | Execution outcome including events, logs, fee breakdown, and substate diff             |

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
