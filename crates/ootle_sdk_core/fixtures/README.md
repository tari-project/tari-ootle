# Golden-vector fixtures

Language-neutral `input → expected output` fixtures for `ootle-sdk-core` (Shared Contract E). The
core is the **single source of truth**: a generator drives it to produce `expected`, and a Rust
runner re-runs the core and asserts it reproduces those bytes **byte-for-byte**.

**These same files are consumed by every host SDK** — Go first, then Kotlin, then TS / Python (D4).
So the format contains **no Rust-only constructs**: scalars, hex strings, nested objects only.

## Layout

One JSON file per vector, under `fixtures/<group>/<name>.json` (clean diffs, easy for host SDKs to
pick up). The runner loads **every** `*.json` under `fixtures/` recursively.

| File | Purpose |
|------|---------|
| `public_transfer/sample_single_key_basic.json` | Sample vector — a single-key public transfer with tiny fixed keys. **Obviously a sample** (`name` starts with `sample/`). The real, cross-validated transfer vector is added in step 05. |

## Format

```jsonc
{
  "name": "public_transfer/single_key_basic", // stable id; shown in failure messages
  "schema_version": 1,                          // integer; bump on format changes
  "provenance": {                               // how/when produced — traceability, not silent magic
    "core_version": "0.33.5",
    "git_rev": "…",                             // git HEAD at generation (or "unknown")
    "generated_by": "ootle-sdk-core golden-vector generator"
  },
  "operation": "build_and_encode_public_transfer", // the core entry point exercised
  "input": {                                    // *exactly* the arguments to that function
    "network": "esmeralda",
    "intent": { … },                            // the typed PublicTransferIntent (serde)
    "keys": {                                   // pinned, deterministic key + nonce material
      "account_secret": "<hex>",
      "auth_nonce": "<hex>",                     // pinned nonce secret (auth signature)
      "seal_nonce": "<hex>",                     // pinned nonce secret (seal signature)
      "seal_secret": "<hex>"                     // OPTIONAL — present only for a separate seal signer
    }
  },
  "expected": {                                 // GENERATOR-OWNED, byte-exact outputs
    "encoded_transaction": "<hex>",             // canonical encoding: lowercase hex
    "transaction_id": "<hex>"                   // 32-byte id, lowercase hex
  }
}
```

### Conventions

- **Bytes are lowercase hex with no `0x` prefix.** Empty buffer = `""`, absent optional = JSON
  `null` (or omitted). This mirrors the Python SDK fixtures so a future Go / Kotlin / Python runner
  reads the same strings.
- **`encoded_transaction` canonical encoding is lowercase hex.** (Not base64 — one canonical form,
  documented here.)
- **`input` is fully deterministic.** No "current epoch", no RNG. The pinned **nonce secrets** are
  part of `input` (Shared Contract C / ledger R3): without them neither `encoded_transaction` nor
  `transaction_id` is reproducible — the generator would need RNG and the runner would never match.

### `expected` is generator-owned — never hand-edit it

Humans author `input` (in the generator's seed code); the **generator is the only writer of
`expected`**. If you change `input`, regenerate — do not hand-patch the bytes.

## Regenerating

```sh
OOTLE_REGEN_FIXTURES=1 cargo test -p ootle-sdk-core --test golden_vectors regen_fixtures
```

The generator seeds the sample vector if it is absent, then re-runs the core over every fixture's
`input` and rewrites `expected` + `provenance`. Regeneration is **idempotent**: with no core change,
it produces byte-identical JSON and leaves the working tree clean (enforced by the
`regen_is_idempotent` test, which pins `git_rev` to a fixed value on both sides of the compare so
only the produced bytes are checked — provenance metadata is allowed to drift with `HEAD`).

To pin `git_rev` (e.g. in CI), set `OOTLE_FIXTURE_GIT_REV=<rev>`; otherwise it is taken from
`git rev-parse HEAD`, falling back to `"unknown"`.

## Running the runner

```sh
cargo test -p ootle-sdk-core --test golden_vectors run_golden_vectors
```

The runner asserts **byte-for-byte on the raw hex strings, never on parsed structures** — a parsed
compare would hide the exact CBOR/encoding drift this harness exists to catch. On mismatch it panics
with the fixture `name`, the file path, and an expected-vs-actual hex diff. A stale committed
`expected` is a **red build**, not a silent pass.

## Adding a new vector

1. Author the `input` in the generator (`tests/golden_vectors.rs`): add a seed fn like
   `sample_input()` / `sample_fixture_seed()`, and have `regen_fixtures` write it to a new
   `fixtures/<group>/<name>.json` if absent (mirror the sample's seed-if-missing block).
2. Run the generator (above) to fill `expected` + `provenance`.
3. Run the runner — it picks up the new file automatically (it globs `*.json`).
4. Commit the JSON. **Do not hand-edit `expected`.**
