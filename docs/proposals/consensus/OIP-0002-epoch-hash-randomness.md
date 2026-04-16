# OIP-0002: Epoch Hash as On-Chain Randomness Beacon

```
OIP Number: 0002
Title: Epoch Hash as On-Chain Randomness Beacon
Status: Draft
Author(s): Tari Ootle Contributors
Created: 2026-04-14
```

## Abstract

This proposal exposes the per-epoch hash — already present in every block header and
derived from the base layer (L1) — as a deterministic, manipulation-resistant randomness
source available inside template execution via `Consensus::current_epoch_hash()`. It also
describes a pattern for epoch-based betting and delayed-settlement use cases using the
**keeper pattern**, which allows any third party to settle an expired bet and earn a small
protocol fee for doing so.

## Motivation

Smart contract templates on Tari Ootle frequently need a source of randomness that is:

1. **Deterministic** — all validator nodes must agree on the same value for a given
   transaction.
2. **Unpredictable in advance** — the value must not be knowable at the time a user
   commits to a bet or game outcome.
3. **Manipulation-resistant** — no single validator or small coalition should be able to
   bias the value.

Prior to this proposal, the only consensus-layer value accessible from templates was
`Consensus::current_epoch()`. There was no randomness source that satisfied all three
properties above.

Use cases that require this primitive include:

- **Betting and prediction markets** — a user places a bet in epoch N and the outcome is
  determined by a value that cannot be known until epoch N+1 begins.
- **Lotteries and raffles** — ticket sales close at epoch N; the winning ticket is drawn
  using the epoch N+1 hash.
- **Fair NFT mints** — trait assignment happens after the mint closes, using the next
  epoch's hash to shuffle allocations.
- **Commit-reveal games** — players commit hidden moves in epoch N and reveal them in
  epoch N+1 with the epoch hash as tiebreaker.

## Specification

### New virtual substate: `CurrentEpochHash`

A new variant is added to `VirtualSubstateId` and `VirtualSubstate` in
`crates/engine_types/src/virtual_substate.rs`:

```rust
pub enum VirtualSubstateId {
    CurrentEpoch,
    CurrentEpochHash,   // new
}

pub enum VirtualSubstate {
    CurrentEpoch(u64),
    CurrentEpochHash([u8; 32]),  // new
}
```

This value is injected by the validator node before transaction execution alongside
`CurrentEpoch`, using the `epoch_hash` field of the block header being proposed/validated.

### Source of the epoch hash

The `epoch_hash` field is already present on every `BlockHeader`
(`crates/storage/src/consensus_models/block_header.rs`). It is defined as:

> "A hash given by the epoch oracle. E.g. the base layer epoch oracle gives the first
> block hash of the epoch."

Key properties:

- Derived from the **L1 base layer block hash** that triggers the epoch transition.
- **Fixed for all blocks within an epoch** — every block in epoch N carries the same
  `epoch_hash`.
- **Unknown until the epoch begins** — it depends on which L1 block is mined at the
  epoch boundary, which is not predictable in advance.
- **Agreed upon by all validators** — it is part of the block header that every validator
  signs over; any disagreement causes a consensus failure as with any other header field.

### Template API

A new method is added to the `Consensus` module in `crates/template_lib/src/consensus.rs`:

```rust
/// Returns the epoch hash of the current epoch.
///
/// The epoch hash is derived from the base layer (L1) block hash at the start of
/// the epoch. It is fixed for the entire epoch and is unpredictable before the
/// epoch begins, making it suitable as a deterministic randomness seed.
pub fn current_epoch_hash() -> [u8; 32] { ... }
```

### Execution pipeline changes

`epoch_hash: FixedHash` is threaded through the following call chain so that each
transaction execution receives the correct value:

```
BlockTransactionExecutor::execute(transaction, epoch, epoch_hash, inputs)
  └─ ConsensusTransactionManager::execute(epoch, epoch_hash, pledged)
       └─ ConsensusTransactionManager::prepare(store, ..., epoch_hash, ...)
            └─ execute_or_fetch(store, ..., epoch, epoch_hash)
```

The value originates from `EpochState::epoch_hash` (already populated from the epoch
manager) in `on_propose.rs`, and from `block.epoch_hash()` in
`on_ready_to_vote_on_local_block.rs`.

### Betting contract pattern

The recommended pattern for an epoch-based betting contract is:

```rust
#[template]
mod betting {
    use tari_template_lib::prelude::*;

    pub struct BettingComponent {
        placement_epoch: u64,
        bet_amount: Amount,
        vault: Vault,
    }

    impl BettingComponent {
        /// Called in epoch N. Records the bet and the current epoch.
        pub fn place_bet(funds: Bucket) -> Component<Self> {
            let placement_epoch = Consensus::current_epoch();
            Component::new(Self {
                placement_epoch,
                bet_amount: funds.amount(),
                vault: Vault::from_bucket(funds),
            })
            .create()
        }

        /// Called in epoch N+1 (by the user or a keeper).
        /// Uses the epoch N+1 hash — unknown at placement time — as the outcome seed.
        pub fn settle_bet(&mut self, keeper_fee_vault: &mut Vault) {
            let current_epoch = Consensus::current_epoch();
            assert!(
                current_epoch > self.placement_epoch,
                "Bet cannot be settled in the same epoch it was placed"
            );

            let seed = Consensus::current_epoch_hash();
            let outcome = seed[0] % 2; // 0 = lose, 1 = win

            if outcome == 1 {
                // pay out winnings
            } else {
                // house keeps stake
            }
        }
    }
}
```

### Settlement: the keeper pattern

Because template execution is always user-initiated, there is no native scheduling
mechanism to trigger settlement automatically. This proposal adopts the **keeper pattern**:

- The settlement function (`settle_bet`) is callable by **anyone**, not just the original
  bettor.
- A small **keeper fee** is paid from the bet pool to whoever calls `settle_bet` once the
  settlement epoch has arrived.
- External **keeper bots** watch the chain for unsettled bets and race to settle them,
  earning the fee as compensation.
- The bettor retains the right to call `settle_bet` themselves at no fee if they prefer.

This approach requires no protocol changes beyond what is already implemented and is
well-established in the DeFi ecosystem (e.g. Chainlink Keepers, Gelato Network).

The fee incentive structure should be tuned by the template author. A reasonable default
is 0.5–1% of the bet amount, sufficient to cover gas costs plus a small profit margin
for the keeper.

## Rationale

### Why the epoch hash and not a VRF or threshold signature?

A VRF (Verifiable Random Function) or threshold BLS signature scheme would provide
stronger randomness guarantees (in particular, protecting against last-revealer attacks
by the leader). However:

- These require significant cryptographic infrastructure changes to the validator node
  and consensus protocol.
- The epoch hash already exists and requires zero additional consensus overhead.
- The manipulation surface is small: a dishonest leader can choose not to propose a block
  (at the cost of losing their leader reward), but cannot choose the epoch hash itself,
  which is fixed by the L1. Epoch-level granularity further reduces the attack surface
  compared to per-block randomness.

For use cases where bet sizes are moderate, the epoch hash provides sufficient security.
High-value applications should wait for a dedicated VRF-based randomness beacon (future
work).

### Why not expose `get_epoch_hash(epoch)` for arbitrary past epochs?

Querying historical epoch hashes would require pre-injecting them as virtual substates
before execution (since the WASM sandbox has no async call-out capability). This creates
a chicken-and-egg problem: the executor cannot know which historical epochs a template
will query before running it. A sliding-window pre-injection (last N epochs) is possible
but wasteful. This is left as future work; the current proposal satisfies the primary
use case (settle in epoch N+1) cleanly.

### Why not a native scheduling primitive?

True auto-settlement would require a new `Command::ScheduledExecution` type, persistent
scheduled-execution storage, and block proposal logic to query and include due executions.
This is architecturally significant and touches consensus, the state store, and the
template API simultaneously. The keeper pattern achieves equivalent user-visible behaviour
with no protocol changes and is therefore preferred for an initial implementation.

## Backwards Compatibility

The changes are **fully backwards compatible**:

- `VirtualSubstateId` and `VirtualSubstate` gain new variants but all existing match
  arms and deserialization paths are unaffected.
- The `BlockTransactionExecutor::execute` trait gains a new parameter
  (`current_epoch_hash: FixedHash`). All existing implementors must be updated; there is
  currently only one production implementor
  (`TariBlockTransactionExecutor` in `applications/tari_validator_node`).
- `Consensus::current_epoch_hash()` is a purely additive API; no existing template code
  is affected.
- The `ConsensusAction` enum gains a new variant. Templates compiled against the old ABI
  will not call it; templates compiled against the new ABI require validator nodes that
  implement it. A validator node that does not implement `GetCurrentEpochHash` will return
  a `VirtualSubstateNotFound` error, causing the transaction to be rejected.

Upgrade path: validator nodes must be upgraded before templates using
`current_epoch_hash()` are deployed. This follows the standard Tari Ootle upgrade
protocol for new engine opcodes.

## Test Cases

The following test cases should be implemented in `crates/consensus_tests/` and
`crates/engine/tests/`:

1. **`test_epoch_hash_is_injected`** — execute a transaction that calls
   `Consensus::current_epoch_hash()` and assert the returned value matches the
   `epoch_hash` field of the block header used during execution.

2. **`test_epoch_hash_differs_across_epochs`** — execute two transactions in different
   epochs and assert their `current_epoch_hash()` values differ.

3. **`test_epoch_hash_constant_within_epoch`** — execute two transactions in the same
   epoch (different blocks) and assert their `current_epoch_hash()` values are identical.

4. **`test_settle_bet_requires_later_epoch`** — assert that calling `settle_bet()` in the
   same epoch as `place_bet()` returns an error.

5. **`test_keeper_can_settle`** — assert that a third-party address (not the bettor) can
   successfully call `settle_bet()` and receive the keeper fee.

6. **`test_epoch_hash_not_available_without_injection`** — assert that executing a
   template that calls `current_epoch_hash()` without the virtual substate injected
   returns `VirtualSubstateNotFound`.

## Implementation

The implementation is complete. The following are implemented:

| Component                                                     | File                                                                                                                  | Status |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------ |
| `VirtualSubstateId::CurrentEpochHash`                         | `crates/engine_types/src/virtual_substate.rs`                                                                         | Done   |
| Injection in validator node executor                          | `applications/tari_validator_node/src/consensus/block_transaction_executor.rs`                                        | Done   |
| `epoch_hash` threaded through consensus                       | `crates/consensus/src/hotstuff/transaction_manager/manager.rs`, `on_propose.rs`, `on_ready_to_vote_on_local_block.rs` | Done   |
| `get_current_epoch_hash()` in runtime                         | `crates/engine/src/runtime/working_state.rs`, `tracker.rs`, `impl.rs`                                                 | Done   |
| `Consensus::current_epoch_hash()` template API                | `crates/template_lib/src/consensus.rs`                                                                                | Done   |
| Injection in indexer dry-run                                  | `applications/tari_indexer/src/dry_run/processor.rs`                                                                  | Done   |
| Default value in test tooling                                 | `crates/template_test_tooling/src/template_test.rs`                                                                   | Done   |
| `remove_virtual_substate()` helper in test tooling            | `crates/template_test_tooling/src/template_test.rs`                                                                   | Done   |
| Reference betting template (`EpochBettingHouse` + `EpochBet`) | `crates/engine/tests/templates/epoch_betting_house/`, `crates/engine/tests/templates/epoch_betting/`                  | Done   |
| Engine test cases 1–6 from this OIP                           | `crates/engine/tests/test.rs` (mod consensus), `crates/engine/tests/epoch_betting.rs`                                 | Done   |
| Epoch hash mixed into `IdProvider::new_uuid()`                | `crates/engine_types/src/id_provider.rs`, `crates/engine/src/runtime/working_state.rs`                                | Done   |
| Developer documentation for template authors                  | `docs/developer-docs/epoch-hash-randomness.md`                                                                        | Done   |

### Betting template design note

The reference implementation uses a two-component architecture:

- **`EpochBettingHouse`** — a long-lived house component holding a liquidity reserve. The
  operator funds it with initial capital. `place_bet` validates that the reserve can cover
  the potential win payout (`2 × stake`) and creates a new `EpochBet` via `TemplateManager`.
- **`EpochBet`** — a per-bet component holding the player's stake. On settlement it calls
  back into the house via `ComponentManager` to either pay the winner (house matches the
  stake) or deposit the losing stake into the house reserve.

Losing stakes are returned to the house reserve (not burnt) so the house accumulates profit
from losses and can sustain payouts on wins without requiring additional operator capital.

## References

- [Block header definition](../../crates/storage/src/consensus_models/block_header.rs) — `epoch_hash` field
- [EpochState](../../crates/consensus/src/hotstuff/epoch_state.rs) — source of `epoch_hash` in the consensus worker
- [VirtualSubstate definition](../../crates/engine_types/src/virtual_substate.rs)
- [Consensus template API](../../crates/template_lib/src/consensus.rs)
- [Chainlink Keepers documentation](https://docs.chain.link/chainlink-automation) — reference keeper pattern implementation

## Copyright

This document is released under the BSD 3-Clause License.
