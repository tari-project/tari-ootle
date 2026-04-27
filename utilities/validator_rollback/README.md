# `tari_validator_rollback`

Offline break-glass rollback tool for Tari Ootle validator nodes.

Run against a **stopped** validator's RocksDB directory to roll state back to a prior
`EpochCheckpoint` and produce an audit file of everything that changed. Authorization is
filesystem access — no keys, no signatures, no admin RPC. The resume path is normal
validator startup: `create_genesis_block_if_required` rebuilds genesis from the truncated
state tree.

## Install

From the workspace root:

```bash
cargo build --release -p tari_validator_rollback
./target/release/tari_validator_rollback --help
```

The tool must be paired with a matching validator binary — they share the RocksDB schema.

## Subcommands

### `apply` — perform (or dry-run) a rollback

```bash
tari_validator_rollback apply \
    --state-db /var/lib/tari/validator/rocksdb \
    --target-epoch 42 \
    [--shard-group 0:63] \
    [--audit-out PATH] \
    [--dry-run]
```

- `--state-db` — path to the validator's RocksDB dir. RocksDB's LOCK file enforces that
  the validator process is stopped; a running validator will cause `apply` to fail with
  `lock hold by another process`.
- `--target-epoch` — the epoch to roll back to. The validator must hold a local
  `EpochCheckpoint` for this epoch (it only stores checkpoints for shard groups it
  participated in). A validator can't roll back to an epoch it didn't participate in.
- `--shard-group START:END` — only needed when a validator's DB contains multiple
  checkpoints at the target epoch (e.g. it moved shard groups). The tool errors with
  the candidate list if this is ambiguous.
- `--audit-out PATH` — where to write the audit file. Default:
  `./rollback-audit-<target_epoch>-<unix_secs>.bin`.
- `--dry-run` — generate the audit file without mutating the state store.

**What apply does (in a single write transaction):**

1. Resolves the local `EpochCheckpoint` at `target_epoch`.
2. Walks state transitions at `version > checkpoint_version` for each shard and
   per-block finalising transactions at `epoch > target_epoch`, writing the audit.
3. (Unless `--dry-run`) calls `state_tree_truncate_to_version`,
   `substates_rewind_to_state_version`, `rollback_delete_after_epoch` per shard, and
   records a `rollback_history` breadcrumb (`target_epoch`, `shard_group`,
   `applied_at_unix_secs`, `tool_version`, `audit_file_basename`).

### `inspect` — human-readable audit summary

```bash
tari_validator_rollback inspect --audit ./rollback-audit-42-1735000000.bin
```

Prints header (target epoch, tip-before, shard group, state versions per shard, tool
version, dry-run flag) and footer counts (substates removed, substates rewound,
substate transitions, transactions unfinalised, blocks deleted).

### `convert` — re-serialise as JSON or JSONL

```bash
# One JSON object per line — good for `jq`:
tari_validator_rollback convert --audit rollback-audit.bin --format jsonl | jq

# Single JSON document — header + arrays + footer:
tari_validator_rollback convert --audit rollback-audit.bin --format json --out audit.json
```

## Audit file format

Binary, length-prefixed borsh stream:

```
[u32 LE magic "TARR"]
[u8  format_version = 1]
[u8  reserved][u16 LE reserved]
stream of: [u32 LE record_len][borsh(AuditRecord)]
```

`AuditRecord` variants:

- **`Header`** — target epoch, shard group, pre-rollback tip, state-version-per-shard map, generation timestamp, tool version, dry-run flag.
- **`SubstateSummary`** — one per distinct affected substate: `substate_id`, `shard`, action (`Removed` / `Rewound`), pre- and post-rollback version.
- **`SubstateTransition`** — one per state transition being reverted (in reverse-application order): `substate_id`, `shard`, `state_version`, `Up`/`Down`, `epoch`.
- **`TransactionUnfinalised`** — one per transaction whose finalising block sits at `epoch > target_epoch`: `transaction_id`, `finalised_in_block`, `finalised_at_epoch`.
- **`Footer`** — running totals, useful as a quick impact summary.

Consumers that want typed access can depend on this crate as a library (the types are
exposed via `tari_validator_rollback::audit`) instead of converting to JSON.

## Operator workflow

For each validator in the committee:

```bash
# 1. Stop the validator.
systemctl stop tari-validator

# 2. Dry-run first, review the audit, confirm impact is what you expect.
tari_validator_rollback apply \
    --state-db /var/lib/tari/validator/rocksdb \
    --target-epoch 42 \
    --dry-run \
    --audit-out /var/lib/tari/validator/rollback-audit-42-dryrun.bin
tari_validator_rollback inspect \
    --audit /var/lib/tari/validator/rollback-audit-42-dryrun.bin

# 3. Apply for real.
tari_validator_rollback apply \
    --state-db /var/lib/tari/validator/rocksdb \
    --target-epoch 42 \
    --audit-out /var/lib/tari/validator/rollback-audit-42.bin

# 4. Archive the audit alongside the incident ticket.
cp /var/lib/tari/validator/rollback-audit-42.bin /srv/incident-2026-04-24/VN1.bin

# 5. Restart the validator — consensus resumes via normal startup.
systemctl start tari-validator
```

Ordering matters for quorum reformation: coordinate the stop/apply/start across all
validators in the committee.

## Caveats

- **Indexers and wallets need re-bootstrap** after a rollback. The audit file's
  `transaction_unfinalised` records list what their state machines should invalidate;
  the actual resync is operator-driven.
- **The tool must match the validator binary's schema.** Ship the matching release
  together; mixing versions across a migration is undefined.
- **No retention guarantees** on the audit file. The tool writes to the working
  directory by default; the DB's `rollback_history` CF only records the filename,
  not the contents — losing the file means losing detail beyond the breadcrumb.
- **No idempotency key.** Running the tool twice with the same `--target-epoch`
  produces two history rows; the second run's storage ops are effectively no-ops
  against the already-truncated DB.
