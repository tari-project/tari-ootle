# tari_consensus_types

Core type definitions for the Tari Ootle BFT consensus protocol.

This crate contains the shared data structures used across the consensus layer, including block and certificate
identifiers, voting types, bookkeeping state, and transaction decisions.

## Modules

| Module | Contents |
|---|---|
| `ids` | `BlockId`, `PcId`, `TcId`, `QcId` — hash-based identifiers for blocks and certificates |
| `certificates` | `QuorumCertificate`, `ProposalCertificate`, `TimeoutCertificate`, and their associated votes |
| `bookkeeping` | Consensus state tracking: `HighPc`, `HighTc`, `LeafBlock`, `LockedBlock`, `LastExecuted`, `LastVoted`, etc. |
| `decision` | `Decision` enum — `Commit` or `Abort` outcome for a transaction |
| `types` | `AccumulatedData` — aggregated data accumulated during consensus rounds |
| `validator_signature` | Validator signature types for signing consensus messages |

## Features

- **`ts`** — Generates TypeScript type definitions via `ts-rs`.
