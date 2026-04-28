# OIP-0002: Substate Schema Versioning via Epoch-Bound Hashing

```
OIP Number: 0002
Title: Substate schema versioning via epoch-bound hashing
Status: Draft
Author(s): Stanley Bondi
Created: 2026-04-23
```

## Abstract

This proposal specifies how Tari Ootle evolves its substate schemas after mainnet without
invalidating existing JellyfishMerkleTree (JMT) hashes and without coordinating a
cross-shard governance event. Schema version becomes a pure function of epoch via a
hardcoded `ProtocolVersion::at(epoch)` table shipped in validator binaries. The epoch of
each substate revision is chained into `hash_substate`'s preimage, transitively binding the
schema version into every committed JMT leaf. On disk, records are wrapped in a
`VersionedSubstateRecord` envelope whose read path lazy-upgrades older generations to the
current in-memory shape, leaving engine code schema-agnostic. New generations are
introduced by freezing the current-generation types under `_VN` names and appending a new
variant to the versioned envelope — both bincode (storage) and borsh (hash preimage)
handle appended enum variants without migrating existing bytes. An outdated binary that
reaches an activation epoch it does not support halts cleanly rather than silently
diverging.

## Motivation

Substate value hashes are committed into per-shard JMTs. After mainnet these hashes must
remain stable forever: any change to the hash preimage of a past substate either
invalidates the committed state tree (losing history) or forces every honest node to
recompute and re-sign every historical hash (a coordinated re-keying of consensus state).
Both are unacceptable.

At the same time the project will, over time, need to change substate layouts and
semantics. Without an explicit mechanism, two validators running different generations of
code can produce byte-identical encodings for semantically different states (positional
encoders like bincode and borsh do not self-describe schemas, and optional-field tricks do
not survive bincode). The resulting divergence is silent — the immediate JMT root agrees,
but execution produces different next-block states and consensus breaks at block _N+1_
rather than _N_.

This OIP reserves a clean activation path for future schema upgrades. The mechanism itself
changes no substate layout today; it wires hooks so that when V2, V3, … are needed, the
activation is a predictable release event rather than a consensus incident.

## Specification

### Overview

1. A new `ProtocolVersion` type in `tari_ootle_common_types` exposes
   `ProtocolVersion::at(epoch) -> SchemaVersion` with a hardcoded, append-only table of
   `(activation_epoch, schema_version)` entries ordered ascending. `SchemaVersion` is an
   explicit enum (`V0`, `V1`, …).
2. `hash_substate` takes the substate's `at_epoch` and chains it into the hash preimage.
   Because schema is a function of epoch and epoch is in the preimage, schema is
   transitively bound into every consensus-committed hash.
3. At runtime, the consensus worker refuses to advance past an epoch whose required
   schema exceeds the binary's `MAX_SUPPORTED`. The error surfaces as
   `HotStuffError::UnsupportedProtocolVersion`.
4. Storage records are wrapped in a `VersionedSubstateRecord` enum. The read path calls
   `into_latest()` which walks the upgrade chain (`upgrade_single_step`) until the
   record is in the latest in-memory shape. Engine code only ever sees the latest type.
5. New schema generations are introduced by freezing the current-generation types under
   `_VN` names and appending a new variant to `VersionedSubstateRecord` (and to any
   affected inner enums, e.g. `SubstateValue`). Both bincode and borsh encode enum
   variants by ascending integer discriminant — appending a variant is backwards-
   compatible; reordering or removing is not.

### Technical Details

#### `ProtocolVersion`

```rust
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SchemaVersion {
    V0 = 0,
    // Future variants appended here only.
}

pub struct ProtocolVersion;

impl ProtocolVersion {
    pub const MAX_SUPPORTED: SchemaVersion = SchemaVersion::V0;

    // Ordered by activation epoch ascending. Entry 0 is genesis.
    // CONSENSUS-BOUND. Never reorder or mutate an entry after it activates on a live network.
    const ACTIVATIONS: &'static [(Epoch, SchemaVersion)] = &[(Epoch(0), SchemaVersion::V0)];

    pub fn at(epoch: Epoch) -> SchemaVersion {
        Self::ACTIVATIONS
            .iter()
            .rev()
            .find(|(at, _)| *at <= epoch)
            .map(|(_, v)| *v)
            .unwrap_or(SchemaVersion::V0)
    }
}
```

Bumping a schema is purely a source change: add a variant to `SchemaVersion`, append a new
row to `ACTIVATIONS`, raise `MAX_SUPPORTED`. The binary-release event _is_ the activation
vote; there is no on-chain governance substate.

#### `hash_substate`

```rust
pub fn hash_substate(substate: &SubstateValue, version: u32, epoch: u64) -> Hash32 {
    substate_value_hasher32()
        .chain(substate)
        .chain(&version)
        .chain(&epoch)
        .result()
        .into_array()
        .into()
}
```

`epoch` is a `u64` at this boundary to avoid a circular dependency between
`tari_engine_types` and `tari_ootle_common_types`; higher layers that hold `Epoch` call
`.as_u64()` at the call site. The field is sourced from `SubstateCreated.at_epoch` of the
revision being hashed — which is part of `SubstateRecord` and already populated by the
state-sync receive path from `SyncStateResponse.epoch`.

#### Runtime enforcement

On every `EpochManagerEvent::EpochChanged`:

```rust
let required_schema = ProtocolVersion::at(epoch);
if required_schema > ProtocolVersion::MAX_SUPPORTED {
return Err(HotStuffError::UnsupportedProtocolVersion { epoch });
}
```

Halt-on-unsupported is safer than best-effort participation: an outdated binary that
cannot correctly emit or validate the new schema must not vote on blocks that exercise it.

#### Versioned record envelope and lazy upgrade

The existing `VersionedSubstateRecord` at
`crates/state_store_rocksdb/src/versioned_types/substate.rs` is the schema-dispatch point
for reads. It implements the `Versioned` trait:

```rust
pub type LatestSubstateRecord = SubstateRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionedSubstateRecord {
    V1(SubstateRecord),
    // V2(SubstateRecord)   // appended when a breaking change lands
}

impl Versioned for VersionedSubstateRecord {
    type Latest = LatestSubstateRecord;
    fn upgrade_single_step(self) -> (Self, bool) { /* walks V1 -> V2 -> ... */ }
    fn into_latest(self) -> Self::Latest { /* applies upgrades until Latest */ }
}
```

All storage reads go through `into_latest()`; every engine call site therefore receives
the latest in-memory representation regardless of which on-disk generation produced it.

Writes always emit the latest variant; historical records are immutable by design (a
mutation creates a new revision, written as the latest variant).

#### Pattern for a schema change

Adding a field to `ComponentHeader` for schema V2 (activating at epoch _N_) looks like
this:

1. Freeze the current generation. Copy today's `ComponentHeader`, `SubstateValue`,
   `SubstateRecord` into `_V1`-suffixed types alongside the current-name types. The
   frozen copies retain exact byte layout; no existing record or hash changes.
2. Modify the current-name types (`ComponentHeader`, `SubstateValue`, `SubstateRecord`)
   to reflect V2's layout. Engine code, which references these by unversioned name,
   automatically sees the latest shape.
3. Point `VersionedSubstateRecord::V1` at the frozen `SubstateRecordV1`, append
   `VersionedSubstateRecord::V2(SubstateRecord)`.
4. Implement `upgrade_single_step(V1) -> V2`, lifting frozen-V1 variants into the
   current-name types and defaulting fields that did not exist at V1.
5. Update `From<SubstateRecord> for VersionedSubstateRecord` to emit `V2` (latest).
6. Add `(Epoch(N), SchemaVersion::V2)` to `ProtocolVersion::ACTIVATIONS`; raise
   `MAX_SUPPORTED` to `V2`.
7. Gate the write-side value of any new field on the current schema so that pre-
   activation proposers emit the default and post-activation proposers emit the new
   value:

   ```rust
   creator_identity: match ProtocolVersion::at(self.current_epoch) {
       SchemaVersion::V0 | SchemaVersion::V1 => RistrettoPublicKeyBytes::default(),
       SchemaVersion::V2 => self.current_caller_identity(),
   },
   ```

No call site outside the storage boundary needs to match on schema version. No historical
record is rewritten. No historical hash changes.

#### State sync interaction

`SyncStateResponse.epoch` (`crates/p2p/proto/rpc.proto:254`) already carries the epoch for
every batch of updates, and the receive path threads it into
`SubstateCreated.at_epoch`. Once epoch is in the hash preimage, the receiver's JMT root
reconstruction is a verification of the peer's epoch claim: any mis-labeled epoch produces
a wrong leaf hash and wrong root, and the sync fails. No wire format change is required.

### Append-only invariants

The mechanism relies on two strict invariants. Both are enforced by convention and
reviewer discipline; neither is enforced by the compiler.

1. **`SchemaVersion` variants are append-only.** Variants are ordered by `#[repr(u32)]`
   value; altering existing values changes every hash derived under them.
2. **`VersionedSubstateRecord` variants are append-only.** Discriminators are positional
   in both bincode and borsh; appending preserves existing variants' bytes, while
   reordering or inserting rewrites them.
3. **`SubstateValue` variants are append-only for the same reason** (in any case where a
   new schema generation requires a new substate type rather than a new field on an
   existing one).

Frozen `_VN` types are also immutable by the same reasoning: modifying a frozen type
silently invalidates on-disk records that carry its bytes.

A test that pins the activations table's ordering and each `SchemaVersion` discriminant
value is added to guard against accidental reordering.

## Rationale

### Why epoch as the schema carrier

An epoch is a globally monotonic clock that every validator computes identically. Making
schema a pure function of epoch gives the consensus layer two things for free:

- **Cross-shard coordination.** Shards do not need to observe a global protocol-version
  substate; each validator derives `ProtocolVersion::at(epoch)` locally and arrives at the
  same answer.
- **No governance substate.** Protocol-upgrade state that might otherwise live in the
  global shard is instead in source code, which already must be distributed and trusted.

Epoch is also already carried through the critical paths:
`SubstateRecord.created.at_epoch` is part of the record, and `SyncStateResponse.epoch`
carries it across the wire. No new field is required anywhere.

### Why not include `SchemaVersion` directly in the hash

Chaining the raw `Epoch` rather than `ProtocolVersion::at(epoch)` keeps the hash keyed on
the load-bearing primitive. If an activation entry is added in error and corrected before
it activates on a live network, no committed hash changes; only the derived-view function
is fixed. Were the hash chained on the derived view, any such correction would rewrite
every hash computed under the mistaken mapping.

### Why append-only rather than a self-describing encoding

Both bincode and borsh are positional. Adopting a self-describing encoding (CBOR,
protobuf) at the storage or hash boundary would allow in-place struct evolution via
optional fields but would also change every existing hash the day it landed — exactly
what this OIP exists to avoid. The append-only discipline is cheap (no new infrastructure)
and compatible with the current encoders.

### Why freeze types instead of in-place struct mutation

Bincode and borsh both decode structs positionally: adding or reordering a field on
`ComponentHeader` invalidates every record and hash that used the prior layout. Freezing
the current-generation type and introducing a new one preserves the bytes of every record
that was written under the frozen shape. The `VersionedSubstateRecord` envelope ensures
this verbosity stays at the storage layer and does not leak into engine code.

### Why a runtime halt on unsupported schema

A validator running an outdated binary that cannot correctly emit or validate a newer
schema must not participate in consensus — anything it produces risks diverging from
honest peers. Halting with an explicit error is operator-friendly and removes the class of
incident where an out-of-date fleet silently corrupts state.

### Alternatives considered

**On-chain `ProtocolVersion` substate bumped by a governance transaction.** Rejected for
this stage because it requires cross-shard observation of the global shard's state during
proposal emission and adds a new governance surface. Software releases are already a
coordinated event; binding activation to releases simplifies operations without losing
auditability (the activations table is in the source tree).

**A per-substate `schema_version` field on the wire and in the hash.** Discussed but
rejected once it became clear that `SyncStateResponse.epoch` already carries per-response
epoch and `SubstateCreated.at_epoch` already lives in the record. A per-substate field
would duplicate information already present.

**`EpochChange` sentinel message in the state-sync stream.** Not required for the same
reason — the existing per-response epoch suffices and is already ordered by state version
(which is also epoch-ordered).

**One-shot storage migration at activation time (re-encoding V1 bytes into V2 bytes).**
Rejected because the migration would recompute every `state_hash`, invalidating every
committed JMT root post-mainnet. The `VersionedSubstateRecord` approach avoids this: V1
bytes stay on disk, V1 hashes stay committed, only the in-memory view is lifted.

## Backwards Compatibility

This OIP's initial landing is backwards-incompatible: introducing the `epoch` parameter
into `hash_substate` changes every existing substate value hash, and therefore every
committed JMT root. This is acceptable because the change lands pre-mainnet. After this
OIP is active, the mechanism it introduces preserves backwards compatibility of historical
hashes across all future schema changes, provided the append-only invariants above are
honored.

Cross-binary compatibility:

- Validators running different `MAX_SUPPORTED` values coexist until an activation epoch
  is crossed. Before the activation, all validators compute `at(current_epoch)` as the
  older schema and agree. At the activation, any validator whose `MAX_SUPPORTED` is below
  the new schema halts — intended behavior, not a compatibility break.
- State sync between a newer sender and older receiver works so long as the receiver's
  `MAX_SUPPORTED` covers the epochs being synced. If not, the receiver halts on the
  earliest unsupported epoch, which is correct (it cannot reconstruct the JMT leaves for
  schemas it does not understand).

## Test Cases

The following tests are required and land alongside this OIP:

1. **Hash binds epoch.** `hash_substate(v, rev, Epoch(0))` ≠ `hash_substate(v, rev,
   Epoch(1))` for the same `(v, rev)`.
2. **Hash is stable across identical inputs.** `hash_substate(v, rev, e) ==
   hash_substate(v, rev, e)`.
3. **Version counter still binds.** `hash_substate(v, 0, e)` ≠ `hash_substate(v, 1, e)`.
4. **`ProtocolVersion::at` genesis and ceiling.** `at(Epoch(0))` is the genesis schema;
   `at(Epoch(u64::MAX))` equals `MAX_SUPPORTED`.
5. **Activations table monotonic.** The table is sorted ascending by activation epoch;
   discriminant values of `SchemaVersion` match their position.
6. **Versioned record round-trip.** `SubstateRecord -> VersionedSubstateRecord -> bytes
   -> VersionedSubstateRecord -> Latest` reconstructs an equivalent record.

At every future schema bump, the tests required are:

7. **Frozen-type byte stability.** A test vector of serialized bytes for the frozen
   `_VN` types, captured at freeze time, must decode identically forever.
8. **Upgrade step correctness.** `upgrade_single_step(VN) -> VN+1` populates defaults for
   fields that did not exist at `VN` and preserves everything else.
9. **State-sync self-validation.** A receiver streaming records written across a schema
   activation must reconstruct a JMT root matching the sender's committed root.

## Implementation

Initial implementation landed in PR #2059

- Adds `crates/common_types/src/protocol_version.rs` with `ProtocolVersion` and
  `SchemaVersion`.
- Threads `epoch` into `hash_substate`, `Substate::to_value_hash`,
  `SubstateValueOrHash::to_value_hash`, `SubstateData::to_value_hash`,
  `SubstateUpdateProof::to_tree_change`, `SubstateChange::to_tree_change`,
  `calculate_state_merkle_root`, and `DiffSummary::from_diff`.
- Adds `HotStuffError::UnsupportedProtocolVersion` and the runtime check in
  `on_epoch_manager_event` at `crates/consensus/src/hotstuff/worker.rs`.
- Regenerates affected test fixtures.

The `VersionedSubstateRecord` envelope is pre-existing at
`crates/state_store_rocksdb/src/versioned_types/substate.rs` and requires no change for
V1. The freeze-and-append workflow specified above applies to the first post-mainnet
schema bump.

## References

- [`VersionedSubstateRecord`](../../../crates/state_store_rocksdb/src/versioned_types/substate.rs)
- [`hash_substate`](../../../crates/engine_types/src/substate.rs)
- [`ProtocolVersion`](../../../crates/common_types/src/protocol_version.rs)
- [Implementation PR tari-project/tari-ootle#2059](https://github.com/tari-project/tari-ootle/pull/2059)

## Copyright

This document is released under the BSD 3-Clause License.
