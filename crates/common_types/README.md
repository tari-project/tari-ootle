# tari_ootle_common_types

[![Crates.io](https://img.shields.io/crates/v/tari_ootle_common_types.svg)](https://crates.io/crates/tari_ootle_common_types)
[![Documentation](https://docs.rs/tari_ootle_common_types/badge.svg)](https://docs.rs/tari_ootle_common_types)

Shared types used across the Ootle layer-2 protocol — consensus, networking,
wallet, and validator node crates. Defines the core vocabulary for sharding,
epochs, substate addressing, and committee membership.

## Key types

| Type                   | Description                                                                               |
|------------------------|-------------------------------------------------------------------------------------------|
| `SubstateAddress`      | 32-byte address derived from a `SubstateId`; determines which shard owns a piece of state |
| `SubstateRequirement`  | A `SubstateId` paired with an optional expected version, used in transaction input sets   |
| `VersionedSubstateId`  | A `SubstateId` with a concrete version number                                             |
| `Shard` / `ShardGroup` | Shard index and contiguous shard range that a validator committee covers                  |
| `NumPreshards`         | Total number of pre-shards in the network (power of two)                                  |
| `Epoch`                | Monotonically increasing consensus epoch counter                                          |
| `NodeHeight`           | Block height within a shard                                                               |
| `Committee<TAddr>`     | Ordered set of validator addresses and their vote power for a shard group                 |
| `Network`              | Enum distinguishing `Mainnet`, `Stagenet`, `Nextnet`, and `LocalNet`                      |

## License

BSD-3-Clause. Copyright 2026 The Tari Project.
