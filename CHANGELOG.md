# Changelog

All notable changes to this project will be documented in this file.
See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

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
