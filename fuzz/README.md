# Fuzzing the Ootle untrusted-input parsers

[cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) (libFuzzer) harnesses over the
parsing/decoding surfaces reachable from untrusted boundaries: p2p wire decode, transaction/mempool
decode, manifest DSL parsing, WASM template loading, and address/substate string parsing.

This crate is **not** part of the root workspace (it is in the top-level `Cargo.toml` `exclude`
list) and is **nightly-only** — libFuzzer needs sanitizer instrumentation that only `cargo fuzz`
wires up. A normal `cargo build`/`cargo test`/clippy sweep never touches it.

Each target treats **any** `Err` as an acceptable outcome. Only a panic, abort, OOM, or hang is a
finding.

## Setup

```sh
cargo install cargo-fuzz          # one-time
rustup toolchain install nightly  # cargo fuzz uses nightly by default
```

## Targets

| Target | Sink under test | Recommended run flags |
| --- | --- | --- |
| `value_decode` | `tari_bor::Value::decode` unbounded recursion (CBOR) | runs on a 256 KiB stack internally |
| `transaction_decode` | `tari_bor` collection adapters: `with_capacity(n)` from the CBOR length header | `-rss_limit_mb=512 -malloc_limit_mb=64` |
| `substate_id_from_str` | address/substate string parsing → `hex.rs` (regression guard for the multibyte-UTF-8 hex panic) | — |
| `parse_manifest` | manifest DSL lex/parse stack overflow via nested token groups | runs on a 256 KiB stack internally |
| `wasm_validate_code` | `WasmModule::validate_code` (wasmer compile + custom-section/ABI decode) | `-rss_limit_mb=2048 -timeout=25` |

## Running

The convenience script runs every target (or named ones) for a fixed time budget each, applying the
right per-target libFuzzer flags and seeding from `seeds/`:

```sh
# From the repo root:
./fuzz/run_all.sh                       # every target, 60s each
./fuzz/run_all.sh value_decode          # one (or more) named target(s)
MAX_TOTAL_TIME=300 ./fuzz/run_all.sh    # 300s per target
```

Or invoke `cargo fuzz` directly:

```sh
cargo fuzz run transaction_decode fuzz/corpus/transaction_decode fuzz/seeds/transaction_decode \
  -- -rss_limit_mb=512 -malloc_limit_mb=64
```

Reproduce a crash artifact:

```sh
cargo fuzz run <target> fuzz/artifacts/<target>/crash-<hash>
```

## Seed corpora

- `seeds/<target>/` — small, curated, committed inputs. `cargo fuzz`/`run_all.sh` merge these into
  the working corpus on each run.
- `corpus/<target>/` — the evolving corpus the fuzzer grows as it runs. Gitignored (runtime state).

Seeds matter most for targets gated behind a magic-number or lexer check, otherwise the fuzzer
stalls at the gate:

- `wasm_validate_code` — a real compiled template binary, so mutations start from a valid module and
  reach the custom-section + ABI-decode paths.
- `parse_manifest` — a small valid manifest, so mutations explore real DSL structure.
- `substate_id_from_str` — one valid address/key string per kind.

The recursion/alloc targets (`value_decode`, `transaction_decode`) need no seed to find their first
crash, but a seed shortens time-to-first-finding.

## CI

`.github/workflows/fuzz.yml` runs each target for a short budget (default 60s) on PRs that touch the
fuzzed crates or the harnesses, and on pushes to `development`/`main`. A crash uploads the offending
input as a build artifact. Trigger a longer manual run from the Actions tab (`workflow_dispatch`,
`max_total_time` input).

## Notes on the sinks

These harnesses were derived from an audit of untrusted-input parsers. The systemic sinks:

1. **`tari_bor::Value::decode`** — `decode_array`/`decode_map`/the `Tag` arm recurse with no depth
   limit; `MAX_VISITOR_DEPTH` in `walker.rs` only applies after the tree is materialised. Fix:
   thread a depth counter through decode.
2. **`tari_bor` collection adapters** — `with_capacity(n)` trusts the CBOR length header. Fix: bound
   `n` against remaining input length.
3. **`hex.rs` `&str` slicing** — fixed separately (byte-based hex decoding); `substate_id_from_str`
   is the regression guard.

The fuzz targets exercise these sinks; they do not themselves contain the fixes.
