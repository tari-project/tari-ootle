# `scripts/` — release & versioning playbook

This directory is the source of truth for **publishing crates to crates.io** and
**reasoning about version bumps** across the workspace. The two scripts share
state (the crate list lives in `publish_crates.py`) so neither drifts.

If you are an AI agent: prefer running these scripts over hand-editing
`Cargo.toml`s. The output is deterministic and accounts for the workspace
versioning rules below.

---

## TL;DR — the two scripts

```sh
# What's in the publish set, in publish order, with current versions + tiers
./scripts/crate_versioning.py list

# I bumped crate X. Who else needs to bump, and how?
./scripts/crate_versioning.py impact <crate>              # non-breaking (patch)
./scripts/crate_versioning.py impact <crate> --breaking   # breaking (minor)

# Once versions are set, publish to crates.io
./scripts/publish_crates.py                  # what would publish (read-only)
./scripts/publish_crates.py --dry-run        # cargo publish --dry-run each crate
./scripts/publish_crates.py --execute        # actually publish
./scripts/publish_crates.py --from <crate> --execute   # resume after a failure
```

---

## Versioning model (must read once)

Every crate in the publish set is `0.y.z`. Cargo's pre-1.0 SemVer rules:

| Bump kind  | Version change             | Dependent impact                                   |
|------------|----------------------------|----------------------------------------------------|
| **patch**  | `0.y.z → 0.y.(z+1)`        | Auto-picked up by `^0.y` pins. No republish needed. |
| **minor**  | `0.y.z → 0.(y+1).0`        | **Breaks** `^0.y` pins. Every direct dependent updates its pin and republishes. |

Crates fall into one of three tiers (see `publish_crates.py::TIER_LABELS`):

| Tier | Label          | Policy |
|------|----------------|--------|
| 1    | `stable`       | Independent versions, rarely change (e.g. `tari_bor`, `ootle-network`, `ootle_byte_type`, `ootle_serde`, `tari_ootle_address`). |
| 2    | `template/sdk` | Independent versions for the template-authoring API and client SDK (`tari_template_*`, `tari_ootle_template_*`, `ootle-rs`). |
| 3    | `workspace`    | Share `[workspace.package].version` in the root `Cargo.toml`. **Move together.** A breaking bump on any tier-3 crate moves the entire cohort. |

**Key consequence:** if a tier-3 crate has a breaking change, the workspace
version bumps and **every** tier-3 crate republishes — even the ones whose own
public API didn't change. Tier-1/2 crates only republish if they actually
depend on something that moved.

---

## `crate_versioning.py` — bump impact analysis

Subcommands:

### `list`

```sh
./scripts/crate_versioning.py list
```

Prints the publish set in topological order with current version + tier + path.
Sourced from `cargo metadata` (versions) and `publish_crates.py` (order/tier).

### `deps <crate>`

```sh
./scripts/crate_versioning.py deps ootle-rs
```

Lists the crate's direct dependencies that are in the publish set. Dev-only
deps are flagged `[dev]` — they don't force downstream bumps.

### `dependents <crate> [--transitive]`

```sh
./scripts/crate_versioning.py dependents tari_engine_types
./scripts/crate_versioning.py dependents tari_engine_types --transitive
```

Lists crates that depend on `<crate>`. With `--transitive`, includes indirect
dependents (the forward closure across normal edges).

### `impact <crate> [--breaking]`

The killer command. Given a (possibly-breaking) change to `<crate>`, prints
the full bump plan.

```sh
# Worked example from the user prompt
./scripts/crate_versioning.py impact tari_engine_types --breaking
```

For a **breaking** bump, output is structured as:

1. **Tier 3 cohort** — every tier-3 crate's new version (because the workspace
   `[workspace.package].version` moves), plus the exact `version = "0.31" →
   "0.32"` pin updates required in `[workspace.dependencies]`.
2. **Tier 1/2 minor bumps** — the crate itself (and any other independently-versioned
   crate that ends up minor-bumped in this round).
3. **Tier 1/2 pin updates** — independently-versioned crates that don't
   minor-bump themselves but still need to update pins and republish at least
   a patch. The output lists which pin(s) to bump and a *patch vs minor*
   recommendation (patch is safe; minor is required only if the crate's own
   public API re-exposes the upstream's changed types).
4. **Dev-only callouts** — informational; dev-only edges never force a bump.
5. **Suggested workflow** — numbered checklist for the bump → format → publish loop.

Without `--breaking`, the output is a single line: it's a patch bump,
dependents auto-pick-up via `^0.y` and nobody else republishes.

---

## `publish_crates.py` — actual publishing

This script handles the topological publish to crates.io. The crate list,
ordering, and tier tags are defined here — **this file is the single source of
truth.** When you add or remove a published crate, edit only this list;
`crate_versioning.py` reads from it.

Common invocations:

```sh
# See what would publish without doing anything (safe to run any time)
./scripts/publish_crates.py

# Build-test each crate against crates.io without publishing
./scripts/publish_crates.py --dry-run

# Publish everything pending
./scripts/publish_crates.py --execute

# Publish a single crate (also useful to retry a failed one)
./scripts/publish_crates.py -p tari_engine --execute

# Resume after a failure
./scripts/publish_crates.py --from tari_engine_types --execute
```

The script skips crates whose current local version is already on crates.io,
so it is safe to re-run after a partial failure.

Prerequisites:
- `cargo login` (authenticate with crates.io).
- Versions are already set correctly in each `Cargo.toml` and pinned in the
  root `[workspace.dependencies]` block. Run `crate_versioning.py impact …`
  first to figure out which versions need to move.

---

## End-to-end recipe (the loop AI agents should follow)

When asked to bump a crate / cut a release:

1. **Decide the change kind.** Patch (non-breaking) or minor (breaking) — based
   on whether the public API changed shape.
2. **Run the impact analysis:**
   ```sh
   ./scripts/crate_versioning.py impact <crate> [--breaking]
   ```
3. **Apply the bumps the script printed:**
   - If a tier-3 crate moved, update `[workspace.package].version` and every
     `version = "<old>"` pin in `[workspace.dependencies]` for tier-3 crates.
   - For each tier-1/2 crate the script listed, update its own `Cargo.toml`
     `version` (patch or minor as advised) and any pin(s) on the bumped deps.
4. **Format:**
   ```sh
   cargo +nightly-2025-06-25 fmt --all
   ```
5. **Dry-run the publish:**
   ```sh
   ./scripts/publish_crates.py --dry-run
   ```
6. **Commit, push, and publish for real after CI is green:**
   ```sh
   ./scripts/publish_crates.py --execute
   ```

If a publish step fails partway through, fix the issue and resume with
`--from <failed-crate> --execute`. Don't restart from the top — already-published
crates skip themselves automatically, but it's wasted CI time.

---

## Adding or removing a published crate

1. Add the `(name, path, tier)` tuple to `CRATES` in `publish_crates.py` in
   topological order (after its dependencies).
2. Add it to the publish set in `crate_versioning.py::PUBLISH_SET` **wait — no.**
   `crate_versioning.py` re-imports `CRATES` from `publish_crates.py`, so the
   set is derived automatically. Nothing else to update.
3. Run `./scripts/crate_versioning.py list` to confirm the new crate shows up
   with the expected version and tier.
4. Run `./scripts/publish_crates.py --dry-run` to confirm the build works.
