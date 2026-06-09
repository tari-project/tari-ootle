# State Store — Domain Context

Sharp definitions for the read/transaction model of the RocksDB state store. These terms name
seams in the storage layer; use them exactly. (Crystallized during the read-consistency design.)

## Read view

The single standalone read seam: a **point-in-time-consistent**, read-only view over the database,
backed by a RocksDB **snapshot**. Every standalone read (`with_read_tx` / the unified read primitive)
opens a read view; many read views may exist concurrently with the single writer and with each other,
and each sees one consistent committed state for its whole lifetime.

Invariants:
- **Consistent**: two reads through the same view observe the same committed baseline regardless of
  writes committing concurrently.
- **`!Send` by construction**: a read view must not be held open across an `.await` or a network
  stream, because a live snapshot pins SST files (space amplification). Making the type `!Send`
  turns the dominant, accidental form of this hazard (holding a view across an await inside a
  spawned `Send` future) into a compile error rather than a documented discipline. The narrow
  residual — a long *synchronous* hold inside `spawn_blocking` — is not type-enforced and stays a
  review concern.

The read view replaces the previous **dual read surface** (`create_read_tx` returning a
non-isolating transaction-backed reader, plus a separate opaque `snapshot()` primitive used only by
tooling/tests). Those collapse into one honest read seam carrying the full read interface.

## Within-write reads

Reads performed *inside* a write transaction. These are **transaction-backed**, not snapshot-backed,
because the writer must observe its own uncommitted writes (**read-your-writes**) — e.g.
`substates_commit_batch` reads a substate it just wrote earlier in the same batch. Within-write reads
are *not* isolated from concurrent commits and do not need to be: see [single-writer model].

The same read interface serves both a read view and within-write reads by being generic over the
underlying reader (snapshot vs. transaction).

## Read-your-writes

The property that a read inside a write transaction sees that transaction's own uncommitted
mutations. Provided by RocksDB `Transaction::get`. A snapshot does **not** have this property, which
is why [within-write reads] must stay transaction-backed and cannot reuse a [read view].

## Single-writer model

At most one write transaction mutates consensus state at a time (the serial HotStuff worker; GC
tasks write disjoint key-families). Read views never need write-conflict isolation because no second
writer races them; they need only a *consistent* view of committed state. Concurrent *writes* are a
latent risk, explicitly out of scope of the read-consistency work.
