# Tari Watcher

**Features**:

* When `auto_register` is true, submits a base-layer registration the first time the local validator public key is missing from the active set, then leaves that check
* Submits layer-1 registration and exit files the validator writes under `<validator-dir>/<network>/data/layer_one_transactions`
* Restarts the validator process while `auto_start` is `always`
* Alerts on Mattermost and Telegram when those channels are enabled

### Quickstart

Initialize with `tari_watcher init` and start with `tari_watcher start`.

`init` writes `data/watcher/config.toml`: `-b` defaults to `data/watcher/` and `-c` defaults to `config.toml`, and a relative config path is joined onto the base directory. Both paths are relative to the current working directory. Edit that file to turn on Mattermost or Telegram. `channel_config.mattermost.enabled` and `channel_config.telegram.enabled` are `false` in the generated file.

The watcher creates the validator directory and starts `tari_validator_node` itself. The default directory is `validator` in the current working directory (`-v`, `DEFAULT_VALIDATOR_DIR`). The process it spawns is:

```
tari_validator_node --network=<network> -b<validator-dir> --node-grpc=<base_node_grpc_url>
```

The default network is `esmeralda`. On first start the validator writes `<validator-dir>/<network>/config/config.toml` and then exits until `validator_node.fee_claim_public_key` in that file is a wallet account public key. The watcher does not pass a fee-claim key.

### Config and Setup

Defaults with no flags (`cli.rs` and `constants.rs`):

```
- base directory (-b): data/watcher/  (relative to the current directory)
- config file (-c): config.toml, under the base directory
- validator directory (-v): validator  (DEFAULT_VALIDATOR_DIR, relative to the current directory)
- validator binary: target/release/tari_validator_node  (DEFAULT_VALIDATOR_NODE_BINARY_PATH)
- base node gRPC: http://127.0.0.1:12001  (DEFAULT_BASE_NODE_GRPC_URL)
- base wallet gRPC: http://127.0.0.1:12003  (DEFAULT_BASE_WALLET_GRPC_URL)
- registration file: <validator-dir>/<network>/registration.json
```

The two process settings in the generated config:

```
- auto_register: bool, default true. Submit a registration when the node is not active.
- auto_start: "always" (default), "just-once", or "never". "always" starts the validator and restarts it after it exits.
```

There is no `auto_restart` key. `tari_watcher init --no-auto-restart` writes `auto_start = "just-once"`. `tari_watcher init --unmanaged` writes `auto_start = "never"`. `tari_watcher init --no-auto-register` leaves `auto_register` true unless `--unmanaged` is also set.

### Project

```
├── alerting.rs            # Mattermost and Telegram notifiers
├── cli.rs                 # init and start
├── config.rs              # config written by init
├── constants.rs           # default paths and gRPC URLs
├── helpers.rs
├── logger.rs
├── main.rs
├── manager.rs             # validator process supervisor
├── minotari.rs            # base-layer gRPC
├── monitoring.rs
├── process.rs             # spawns tari_validator_node
├── shutdown.rs
└── transaction_worker.rs  # registration and layer-1 transaction submission
```
