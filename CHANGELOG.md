# Changelog

All notable changes to this project will be documented in this file.
See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

## [0.39.3](https://github.com/tari-project/tari-ootle/compare/v0.39.2...v0.39.3) (2026-08-26)

### ⚠️ Upgrade notes

- `feat!` — **Validators and indexers must be upgraded together.** The `sync_state` RPC now takes a list of
  `(shard, start_state_version)` cursors instead of a single shard, and its batch/completion messages
  carry the shard they belong to. The protocol name is unversioned, so a mixed-version pair cannot sync.
- `feat!` — **`tari_indexer_client`**: `TransactionEntry` gains a non-optional `source` field, so
  struct-literal construction breaks for downstream consumers. `ListRecentTransactionsRequest` gains an
  optional `source` filter.
- **Operator note** — indexer `transaction_retention_epochs` now defaults to `Some(50)` instead of retaining
  forever. Transaction rows written before the retention column existed carry epoch 0, so the first pruner
  pass after upgrading clears that backlog — whether or not gossip indexing is enabled.

### Indexer

- `feat` — **Transactions are indexed from network gossip**, not just from direct submissions. The indexer
  joins the transaction gossip topic as a full mesh participant: it validates what it receives, reports a
  verdict that propagates the transaction onward, and stores it. New config `index_gossiped_transactions`
  (default true) and `max_transaction_gossip_queue_bytes` (128 MB); `index_gossiped_transactions` is
  reported on `/info`, and metrics for received/accepted/rejected/ignored/stored/dropped plus queue depth
  are exposed behind the `metrics` feature.
  The stored transaction set is explicitly **best effort** — an indexer misses whatever was gossiped while
  it was offline or its queue was full, and nothing backfills it. Receipts for committed transactions stay
  complete from genesis; transaction bodies and never-committed transactions do not.
- `feat!` — `source` on `TransactionEntry` records where a transaction was learned of, with an optional
  `source` filter on the recent-transactions listing. A direct submission upgrades a row already stored
  from gossip.
- `feat` — **At most two state-sync streams per shard group** instead of one per shard — previously 257
  serialized round-trips per round at `P256`, roughly 21s at 80ms RTT against a 30s work interval.
  Per-shard progress is still recorded as completion markers arrive, so an interrupted stream resumes from
  where it got to. A truncated stream is now an error rather than silent success.
- `fix` — **Default substate rate limit raised** — wallets hit the limiter during a normal poll.

### Consensus

- `feat!` — **`sync_state` streams many shards over one stream.** Cursors must be non-empty, at most
  `num_preshards + 1`, in range, strictly ascending, and start above version 0; the responder streams each
  shard contiguously and closes it with its own marker. Verification granularity is unchanged — checkpoints
  already carry per-shard tree roots. The validator passes a single-element cursor list, so its sync
  behaviour is unchanged; ranged validator sync follows separately.

### Wallet

- `fix` — **Bindings**: `shortenString` returns short strings unchanged instead of producing overlapping
  output such as `Rick Ast...k Astley` in NFT metadata cards. Long addresses, hashes and Substate IDs keep
  their existing format.

### Swarm daemon

- `fix` — **Mining stops at the validator activation epoch.** Mining a fixed 20 blocks after registration
  crossed two epoch boundaries with 7 validators, so no committee ever ran consensus in the activation
  epoch and no checkpoint was ever written for it — every cold-starting validator then looped
  `Syncing → Failure → Sleeping` forever. The daemon now mines to a computed height and polls the validators
  for activation instead of mining further.
- `feat` — **Web UI redesigned around divergence.** A consensus spine shows validators as channels on a
  shared rail at the committee tip, so a lagging validator falls off it by a distance sized to its block
  deficit; a pool matrix replaces the per-node "transactions from other pools" tables. App shell with
  sidebar navigation, a live status bar and pages for validators, wallets, indexers, base layer and
  instances. Every feature of the old UI is kept.
- `fix` — One shared polling loop replaces per-card 1s timers; RPC failures surface as toasts; the log viewer
  gains level filters, search, follow and wrap; instance data deletion calls the method that actually
  exists (`delete_data`); reading `final_decision` no longer throws on unfinalized transactions;
  `npm run dev` proxies to the daemon.

### Build & CI

- `build` — **`tari_comms`, `tari_core` and `tari_p2p` are gone from the walletd, indexer and validator node
  dependency graphs.** `minotari_app_grpc` is now depended on with `default-features = false`; only its
  generated protobuf types were ever used. `tari_watcher`, `tari_swarm_daemon` and `integration_tests`
  still pull the wrapper grpc client crates, so a full `--workspace` build still unifies the feature on.
- `ci` — The `windows-arm64` binary build enters the `amd64_arm64` MSVC developer environment, so
  `liblmdb-sys`' bare `cl.exe` fallback resolves.

### Docs

- `docs` — New template-testing tutorial covering account setup, epoch and epoch-hash overrides, direct
  component-state inspection and rejected-transaction error assertions, backed by executable engine tests
  and a snippet-drift check.

### Crate versions

`[workspace.package].version` moves to `0.39.3` (the whole tier-3 cohort). Independently versioned crates
affected by the breaking `tari_indexer_client` change:

| crate | version |
|---|---|
| `tari_indexer_client` | 0.39.0 → 0.40.0 |
| `ootle-rs` | 0.20.0 → 0.21.0 |
| `tari_ootle_wallet_sdk` | 0.40.1 → 0.41.0 |
| `tari_ootle_wallet_storage_sqlite` | 0.40.0 → 0.41.0 |
| `tari_ootle_walletd_client` | 0.40.0 → 0.41.0 |

## [0.39.2](https://github.com/tari-project/tari-ootle/compare/v0.39.1...v0.39.2) (2026-08-24)

A hardening release: several Byzantine-reachable liveness bugs in consensus, a privilege escalation in
the wallet daemon, two engine metering/scoping gaps, and storage read paths that returned data from
branches or tables they should never have seen.

### ⚠️ Upgrade notes

- **A RocksDB migration (`v1`) runs on first start.** The substate-lock substate-id index now encodes its
  table prefix, so existing entries are rewritten. The migration is idempotent and safe to interrupt;
  a fresh database skips it. Timing is logged.

### Consensus

- `fix` — **Proposal votes are aggregated per voted block.** Votes were bucketed by `(epoch, height)` only,
  but a `ProposalVote`'s signature binds `(block_id, decision)` and its `block_height` is
  attacker-controlled. One Byzantine member voting for an invented `block_id` at the current height got
  its signature folded into the honest block's certificate, which every peer then rejected — repeatable
  every height, halting the chain well below the fault threshold. Safety was never at risk.
- `fix` — **The zero-block QC exemption requires genuine genesis shape.** It keyed off an all-zero header
  hash alone, so a peer could skip signature and quorum validation entirely with an arbitrary
  height/parent and push a receiving validator into `FallenBehind` catch-up. It now also requires a
  `ProposalCertificate` at height 0 with a zero parent; timeout certificates are never exempt.
- `fix` — **`MissingTransactionsRequest` is capped at 1000 transaction ids**, mirroring the bound the
  response path already applied. One small request could otherwise run an unbounded number of blocking
  store lookups inline on the consensus worker thread and echo back a correspondingly huge reply.

### State store

- `fix` — **Range query scans are bounded to their logical table.** Logical tables share a physical column
  family, separated by a leading prefix byte, and two of the four range methods left one end open.
  **Epoch GC therefore never made progress**: a single stored foreign proposal failed the scan, rolling
  back the whole cleanup transaction — block, QC and finalized-transaction pruning included — so the
  database grew without bound.
- `fix` — **Block diff queries are scoped to the queried branch.** The branch filter tested the query's own
  argument rather than the block that recorded the entry, so pending substate state from forked-out
  branches leaked into the evaluation of blocks on the surviving branch — the root cause of the flaky
  `catch_up_rewind_below_leaf_recovers` failure. Same-version changes now order `(version, is_down)`, so a
  DOWN supersedes its UP rather than the winner depending on block-id iteration order.
- `fix` — **The committed substate lock lookup returns the most recent lock**, not an arbitrary index match
  that could be superseded and then feed `try_lock`'s conflict decisions.
- `fix` — **The substate-lock substate-id index carries its table prefix** (migration `v1`, above). Latent
  until a new `SubstateId` variant reached the prefix range, at which point it would have silently
  overlapped another table.
- `fix` — `parked_block_remove_missing_transaction` uses the query-aware key iterator, so a decode error is
  propagated instead of being read as "still missing".

### Wallet

- `fix` — **`webrtc.start` no longer mints session tokens above the caller's own grant.** It parsed a
  caller-chosen permission set out of the request body and signed it verbatim — including
  `Permission::Admin`, which satisfies every check in the daemon. An integration holding only the
  deliberately least-powerful `webrtc` scope could escalate to a full Admin bearer token. Requested
  permissions are now filtered through what the caller was actually granted.

### Execution Engine

- `fix` — **`VaultAction::PayFee` bills and caps its stealth verification.** The canonical
  `stealth_transfer` path pre-charged bulletproof and balance-proof verification and enforced
  `max_fee_intent_transfers`; `PayFee` reached the same verification doing neither, so a template could
  loop `vault.pay_fee` against an unfunded vault and run full ZK verification on every validator in the
  committee, free and uncapped.
- `fix` — **Address allocations are scoped to the owning call frame.** They were tracked in one
  transaction-wide map keyed by small sequential integers and checked only for existence. A template a
  victim contract called could brute-force `GetAddress(0..k)`, learn the victim's expected future
  component address and create its own substate there first. Allocations now travel across frames the way
  buckets and proofs already do.
- `fix` — **A refused engine call fails the transaction.** The entrypoint can only answer WASM with a null
  pointer, and a template is free to ignore it, so refusals must be recorded out of band — three of the
  five null paths did not, and the transaction committed with the call's effect never applied. Version
  skew (a template calling an op an older engine cannot map) was the realistic route in.
- `fix` — The engine's own error log is no longer emitted through the metered `emit_log`, so the payer is
  not charged a `RuntimeCall` plus byte rate and a `max_logs` slot for the engine's diagnostic.
- `feat` — **Dispatcher decode panics are rendered from the template definition.** Templates emit a 5-byte
  marker and the engine expands it from the `FunctionDef` it is already invoking: `account` drops 6,691
  bytes (−2.4%). Non-breaking in both directions — an unmarked or unrenderable message passes through
  untouched.
- `perf` — **`template_lib` sheds the `EngineOp` `Debug` table and its last prose panics** (253-byte string
  table plus its match, eight `expect` messages, and a formatted null branch): another −651 bytes on
  `account`, −424 on `state`, −215 on `hello_world`.

### Indexer

- `feat` — **Optional retention window for submitted transactions.** `transaction_retention` (seconds, unset
  by default, so existing deployments are unchanged) and `transaction_prune_interval` (default 3600).
  Only the submitted transaction body and its locally recorded rejection reason are pruned — synced
  receipts are keyed independently and never touched. Pruning is by age alone, including still-pending
  transactions, since never-sequenced spam is the growth this targets. Deletes run in bounded batches so
  SQLite's database-wide write lock is not held over a large backlog.

### Docs & CI

- `docs` — New reference page on reducing template size; the stealth guide covers script-path spends
  (TIP-0006).
- `test` — Cucumber reports which node failed to start, and integration tests run on GitHub-hosted runners.
- `ci` — `cargo machete` runs on a hosted runner with a prebuilt binary instead of holding a self-hosted
  slot to compile a check that takes 0.6s.

## [0.39.1](https://github.com/tari-project/tari-ootle/compare/v0.39.0...v0.39.1) (2026-08-20)

### Release tooling

- `fix` — **`publish_crates.py` had `ootle_serde` in the wrong position**, after a crate that now depends on
  it, so a release run would fail partway through. It moves to just after `tari_bor`, and `check_order()`
  now cross-references the hand-maintained `CRATES` list against `cargo metadata` and aborts before
  anything is uploaded. `ootle_serde`'s versioned dev-dependency on `tari_template_lib` — a cycle that
  aborts packaging — is replaced by a local fixture.

## [0.39.0](https://github.com/tari-project/tari-ootle/compare/v0.38.0...v0.39.0) (2026-08-20)

### ⚠️ Upgrade notes

- `breaking` — **Testnet reset required.** Transaction ids, `TransactionReceipt`, `max_epoch`, the CBOR 128-bit
  encoding and the fee tables all changed — existing transactions are not compatible.
- `breaking` — **Templates must be rebuilt and republished against the latest `tari_template_lib`.** `Amount`'s
  wire format is now minicbor's native integer encoding (compact up to `u64::MAX`, bignum above)
  instead of a two-element digit array. It's smaller, but not backwards compatible — a template built
  on the old lib can't decode an `Amount` from the engine, or produce one it can read.
- `feat!` — **Manifests: the `info!`/`debug!`/`warn!`/`error!` macros are gone** (`Instruction::EmitLog` was
  removed). Logging *inside* a template is unchanged.
- `fix!` — **JS clients no longer patch `BigInt.prototype.toJSON`.** All bigints now serialize as strings,
  consistently — previously small values went out as numbers.

### Wallet 

- `feat!` — **`max_epoch` is now mandatory** on every transaction. `Transaction::builder(network, max_epoch)`
  takes it at construction; the network caps the window at 2160 epochs (~30 days). This change ensures 
	transactions have bounded validity.
- `feat!` — **Stealth transfers can pay up to 16 recipients in one statement** (was 8), which is cheaper than
  splitting across statements.
- `fix` — **Stealth fee estimates now settle before they're reported.** The estimate is priced from the
  transfer's actual shape locally, instead of dry-running at a guessed fee — no more estimates that
  describe a cheaper transaction than the one that gets built, and no extra network round trips.
- `fix` — **`final_fee` reports what was actually paid** (`total_fees_paid`), not the dry-run minimum. Partly
  paid transactions no longer look like they were overcharged.
- `fix` — **NFT transfers**: the wallet waits for local NFT records to update before answering, so a sent NFT
  disappears from your NFT list immediately; the web UI send dialog now stays on its result step
  instead of snapping back to the form.
- `feat` — **Web UI**: indexer liveness pill in the app bar (colour + tooltip with URL/epoch/error).
- `fix` — **Web UI**: the login screen no longer issues authenticated RPCs.
- `feat` — **ootle-wasm**: new `buildScriptPathWitness`, `buildStealthInputsStatementFromInputs` and
  `createTransferStatement` bindings — `PayTo::Conditions` outputs (hashlock/timelock/covenant MAST
  trees) can now be spent from the browser.

### Indexer

- `feat` — **Blob references are validated at ingress**, so malformed blob lists are rejected before signature
  verification instead of being forwarded to committees.
- `fix!` — `tari_indexer_client` / `@tari-project/indexer-client` pick up the bigint-as-string encoding.

### Consensus

- `feat!` — **Transactions have a bounded lifetime.** `max_transaction_validity_epochs = 2160` on every network;
  a transaction can no longer stay sequenceable forever.
- `feat!` — **Transaction ids exclude the seal signature's witness data**, so re-sealing an identical body can't
  produce N distinct valid transactions (no approve-once-execute-many).
- `feat!` — **`TransactionReceipt` gains a 32-byte intent commitment** — you can prove a transaction
  produced a given receipt without revealing the signers' or sealer's public keys.
- `fix!` — The exhaust burn rate is now bounded at build time, so a network can't be configured above
  the rate the fee estimate assumes.
- `chore!` — 128-bit CBOR integers use minicbor's native encoding (wire-breaking for values inside the
  CBOR integer range).

### Execution Engine

- `feat!` — **Publish fees retuned.** Free allowance 30 KiB → 96 KiB plus a flat 250,000 µT per publish. A large
  template (~260 KiB) drops from ~5 tTARI to ~2.7 tTARI; oversized templates stay expensive.
- `fix!` — **Receipts no longer carry `logs`**; use events for anything you need to index.
- `fix!` — **Receipts are now paid for.** The receipt substate is now charged as storage.
- `fix!` — **A log is charged for the bytes it carries**, not a flat per-call fee. Ordinary diagnostics
  cost about what they did; filling `max_logs` with 32 KiB entries no longer does.
- `fix!` — **Finalization fees are charged against the state actually persisted.** A transaction that commits
  only its fee intent is no longer priced against state that gets thrown away.
- `fix!` — **Compute is funded from the payment's unspent balance**, and the fee intent's compute is capped at a
  flat credit — free compute from repeated fee-intent aborts is closed.
- `fix!` — **Confidential accounting**: `total_supply` now tracks confidential commitments (previously
  only the revealed amount, so burnt value was reported forever), and the ElGamal value proof is sound
  (it was forgeable for any claimed value).
- `fix!` — **Engine calls are refused outside a template invocation** — from `tari_alloc`, `tari_free` or the
  response allocation. Nothing legitimate does this; it was a route to unmetered effects and, via
  `tari_alloc`, a node crash.

## [0.3.0](https://github.com/tari-project/tari-ootle/compare/v0.2.0...v0.3.0) (2023-12-19)

### ⚠ BREAKING CHANGES

* libp2p (#827)

### Features

* add version to template
  WASMs ([#835](https://github.com/tari-project/tari-ootle/issues/835)) ([8612eab](https://github.com/tari-project/tari-ootle/commit/8612eab9a1e6a713b04f86e624c5501fcf1c1808))
* do fee estimation in UI
  transfer ([#826](https://github.com/tari-project/tari-ootle/issues/826)) ([93bfd45](https://github.com/tari-project/tari-ootle/commit/93bfd452bd33fe8138d98df164bddbe7642ed650))
*
libp2p ([#827](https://github.com/tari-project/tari-ootle/issues/827)) ([9c29995](https://github.com/tari-project/tari-ootle/commit/9c29995cf0e3f5e7bbb875ea20e02dfa20eab540))
* **p2p:** peer-sync
  protocol ([#844](https://github.com/tari-project/tari-ootle/issues/844)) ([b49af42](https://github.com/tari-project/tari-ootle/commit/b49af421ec3cb72af6df42a952e26eeb4c286c03))
* request foreign
  blocks ([#760](https://github.com/tari-project/tari-ootle/issues/760)) ([7a59c4d](https://github.com/tari-project/tari-ootle/commit/7a59c4d4d2f3d3dcf55880e9a3fd12a5a73dc25e))
* show dummy blocks in
  ui ([#843](https://github.com/tari-project/tari-ootle/issues/843)) ([d5c77f6](https://github.com/tari-project/tari-ootle/commit/d5c77f6e2dbcaa9518343bc453df77c56924e219))

### Bug Fixes

* claim burn in the
  ui ([#841](https://github.com/tari-project/tari-ootle/issues/841)) ([ca80982](https://github.com/tari-project/tari-ootle/commit/ca80982672e4849f52ee5befca8e5e2e7106a003))
* cli argument
  duplicate ([#837](https://github.com/tari-project/tari-ootle/issues/837)) ([cb2d694](https://github.com/tari-project/tari-ootle/commit/cb2d694feb259683a0c58697b6d37d55c6a91867))
* force txs refetch on account change in
  UI ([#833](https://github.com/tari-project/tari-ootle/issues/833)) ([3e09ad5](https://github.com/tari-project/tari-ootle/commit/3e09ad5a2bb00dc4e309a9874f968cd17c34f7ed))
* **p2p/messaging:** single stream per
  connection ([#845](https://github.com/tari-project/tari-ootle/issues/845)) ([c0e09fe](https://github.com/tari-project/tari-ootle/commit/c0e09fefffaee7666c55c36025c039026109f21d))
* **swarm:** exit with error if unsupported seed
  multiaddr ([#836](https://github.com/tari-project/tari-ootle/issues/836)) ([b54bde8](https://github.com/tari-project/tari-ootle/commit/b54bde8178883a49038aa9b0ce6f57450e7184d6))

## [0.2.0](https://github.com/tari-project/tari-ootle/compare/v0.1.1...v0.2.0) (2023-12-08)

### ⚠ BREAKING CHANGES

* foreign broadcast reliability counter (#757)

### Features

* add transaction json download to
  ui ([#815](https://github.com/tari-project/tari-ootle/issues/815)) ([50c0ff5](https://github.com/tari-project/tari-ootle/commit/50c0ff5e5bacbcc2deb221b0cd55f42f61174551))
* disable buttons on send, add result
  dialog ([#813](https://github.com/tari-project/tari-ootle/issues/813)) ([1d146b8](https://github.com/tari-project/tari-ootle/commit/1d146b8190696b58dab6dbdae6abe8132319ea97))
* foreign broadcast reliability
  counter ([#757](https://github.com/tari-project/tari-ootle/issues/757)) ([f0dc999](https://github.com/tari-project/tari-ootle/commit/f0dc99954f634a8ac995a65bf06837edacede808))
* foreign proposal
  command ([#792](https://github.com/tari-project/tari-ootle/issues/792)) ([186b20d](https://github.com/tari-project/tari-ootle/commit/186b20d338cd3ee2c152037a6f4ba806148e44eb))
* **integration_tests:** new test for downed
  substates ([#798](https://github.com/tari-project/tari-ootle/issues/798)) ([5a0c47a](https://github.com/tari-project/tari-ootle/commit/5a0c47af80c5690869be218afdb1415742be4317))
* proper transaction signature and
  validation ([#791](https://github.com/tari-project/tari-ootle/issues/791)) ([e6a1082](https://github.com/tari-project/tari-ootle/commit/e6a108215c6e88a1e79738914aa89489836faf9f))
* set refresh balance interval to 5
  sec ([#819](https://github.com/tari-project/tari-ootle/issues/819)) ([61dfa4d](https://github.com/tari-project/tari-ootle/commit/61dfa4d996854910712b050970fdbc5c18496942))
* show substate version in dan wallet
  ui ([#810](https://github.com/tari-project/tari-ootle/issues/810)) ([89b2879](https://github.com/tari-project/tari-ootle/commit/89b287987109b26da70eed596185145d9f4afe24))
* sort TXs in UI, add
  timestamp ([#804](https://github.com/tari-project/tari-ootle/issues/804)) ([7dad32e](https://github.com/tari-project/tari-ootle/commit/7dad32ec1e8cac548b88d1d0bd4e4fe41d0db89a))

### Bug Fixes

* indexer settings in dan wallet
  ui ([#805](https://github.com/tari-project/tari-ootle/issues/805)) ([068d1ad](https://github.com/tari-project/tari-ootle/commit/068d1ad1a3cd4b9eb1a378694dc9714febca1b85))
*
propagation ([#799](https://github.com/tari-project/tari-ootle/issues/799)) ([ef10627](https://github.com/tari-project/tari-ootle/commit/ef10627ea77af78d9c4799dd115b164f2507e942))
* shard range
  computation ([#796](https://github.com/tari-project/tari-ootle/issues/796)) ([892fe0c](https://github.com/tari-project/tari-ootle/commit/892fe0ce871e6c1a8a9f70d9c51ec196f86cd175))
* shorten string on small
  strings ([#823](https://github.com/tari-project/tari-ootle/issues/823)) ([064c540](https://github.com/tari-project/tari-ootle/commit/064c54067ce09b798022bda2e0bdcbbe7a31bb8e))
* **wallet_daemon_web_ui:** send correct max_fee param on
  transfers ([#795](https://github.com/tari-project/tari-ootle/issues/795)) ([0f07b81](https://github.com/tari-project/tari-ootle/commit/0f07b8161ce6493d76d549fc2fd1b8dd9d38dfd2))
