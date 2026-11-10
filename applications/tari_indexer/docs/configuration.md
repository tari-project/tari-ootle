# Tari Indexer Configuration

## Configuration File Structure

The Tari Indexer uses a TOML configuration file that follows a hierarchical structure. The default configuration file is located at `config/config.toml` relative to the base path, but can be overridden using the `--config` command-line option.

## Configuration Sections

### Common Configuration `[common]`

Basic settings that are shared across Tari applications.

```toml
[common]
# Override configuration from a network-specific section (optional)
#override_from = "esmeralda"

# Data directory. The process sets this from --base-path / TARI_BASE_DIR (default ~/.tari/ootle)
# with the network name appended, for example ~/.tari/ootle/esmeralda.
#base_path = "/path/to/tari/data/esmeralda"
```

**Parameters:**
- `override_from`: Network name to inherit configuration from (e.g., "esmeralda", "stagenet", "localnet")
- `base_path`: Directory for this indexer's data. `--base-path` / `TARI_BASE_DIR` supplies the parent and the network name is appended

### Network Selection

Set the network using the top-level `network` parameter:

```toml
# Network to connect to (required)
network = "esmeralda"  # or "stagenet", "nextnet", "localnet"
```

### Indexer Core Configuration `[indexer]`

Main indexer application settings.

```toml
[indexer]
# Node identity file path (default: "indexer_id.json")
#identity_file = "indexer_id.json"

# Data storage directory (default: "data/indexer")
#data_dir = "data/indexer"

# API server listening address (default: "127.0.0.1:18300")
#api_listen_address = "127.0.0.1:18300"

# Prometheus /_metrics listener, separate from the API server, so metrics can be bound to a
# private interface while the API is public. Only used when the indexer is compiled with the
# metrics feature (enabled by default). (default: "127.0.0.1:18302")
#metrics_listen_address = "127.0.0.1:18302"

# Web UI server address (default: "127.0.0.1:15000")
#web_ui_address = "127.0.0.1:15000"

# Public API URL for web UI (optional, defaults to api_listen_address)
#web_ui_public_api_url = "https://myindexer.example.com:18300"

# How long, in seconds, a shard group waits before reopening its state sync stream after it fails,
# or after a validator that does not follow its tip closes it (default: 60)
#state_scanning_interval = 60

# The longest, in seconds, a validator holds a state sync stream open with no transition to send. The
# stream follows the validator's tip and is reopened once this passes, one completion marker per
# shard. Keepalives show the validator is still there while the stream is quiet, so this only sets
# how often a quiet stream is reopened (default: 600)
#state_sync_stream_deadline = 600

# How often, in seconds, a validator is asked to show it is still there while the state sync stream
# has nothing to send. Each keepalive re-confirms the shards the stream has caught up, which keeps the
# substate cache serving quiet shards. Must be well under state_sync_stream_deadline; validators serve
# no shorter than their own minimum (5s by default) (default: 10)
#state_sync_keepalive_interval = 10

# How many epochs past its terminal epoch a stored transaction is retained.
# A transaction's terminal epoch is the epoch it committed in once its receipt has been indexed, and
# its max_epoch (the last epoch it could still be sequenced in) until then, so a transaction that is
# never sequenced ages out on the same schedule as one that commits. "forever" retains transactions
# indefinitely; 0 keeps only those that can still commit or committed in the current epoch
# (default: 50).
# Applies to every stored transaction, whether submitted here or observed on the gossip topic.
# Only the transaction body and its locally recorded rejection reason are pruned;
# transaction receipts synced from the network follow transaction_receipt_retention_epochs, which
# is kept longer, so a pruned transaction still resolves to its receipt-backed outcome. Set this
# well above the longest a client may take to poll for a result: once pruned, a transaction no
# longer appears in the recent-transactions listing or single transaction lookup. Transactions
# stored before this indexer recorded a terminal epoch carry epoch 0, so the first pass after
# enabling this prunes that entire backlog. Pruning bounds database growth but does not return disk
# to the filesystem: SQLite reuses the freed pages rather than shrinking the file.
#transaction_retention_epochs = 50

# How many epochs past the epoch its transaction committed in a transaction receipt is retained
# (default: 1000). "forever" retains receipts indefinitely. A pruned receipt no longer answers
# a result lookup or appears in the receipt listing. A stored transaction reports its outcome from
# its receipt, so a window not longer than transaction_retention_epochs is raised to one epoch
# longer, and receipts are retained forever while transactions are. The economic totals on
# /network/economics, including the receipt count, keep counting pruned receipts.
#transaction_receipt_retention_epochs = 1000

# How many epochs past the epoch its transaction committed in an event is retained (default: 1000).
# "forever" retains events indefinitely. A pruned event no longer answers event queries, and the
# event stream answers 410 to a resume cursor older than the newest pruned event, so the client
# resynchronises from current state instead of silently missing events.
#event_retention_epochs = 1000

# How many epochs past the epoch a UTXO was spent in its tombstone is retained (default: 504, about a
# week). "forever" retains tombstones indefinitely. Pruning raises the shard's tombstone horizon on
# /info, and a UTXO stream resuming from below it is told its spends are incomplete, so the wallet
# checks the outputs it holds in that shard and still learns of every spend.
#utxo_tombstone_retention_epochs = 504

# How long each pruner idles between passes once it has nothing left to prune, in seconds. While a
# backlog remains it drains in back-to-back batches rather than waiting out this interval. Only used
# when a retention window is set. Also accepted as transaction_prune_interval.
#prune_interval = 3600

# Compiled-template caches. The in-memory one holds modules for the life of the process; the on-disk
# one holds their serialized form under <data_dir>/wasm_cache and is shared by the template manager
# and dry runs. A compiled artifact runs about ten times the size of the WASM it came from, so the
# disk cap is what keeps that directory bounded: over it, the least recently used artifacts are
# deleted and recompiled on demand. Both are node-local caches — deleting either costs CPU, never
# data.
#[indexer.templates]
#max_cache_size_bytes = 1073741824
#max_disk_cache_size_bytes = 10737418240

# Cache TTL, in seconds, for substates fetched during dry-run transaction processing.
# A shorter TTL reduces the chance of stale fee estimates. (default: 10)
#dry_run_cache_ttl = 10

# When true, a substate fetched from validators to serve a client read must come with a proof
# that verifies against the shard group committee, or the read fails. Disabling it serves the
# value unverified, as fetched from a single validator. (default: true)
#verify_substate_proofs = true

# How long, in seconds, after a shard was last confirmed level with its committee the substate
# cache keeps serving entries for it. A shard with no transition is re-confirmed by each
# keepalive on its state sync stream. (default: 300)
#substate_cache_max_serve_lag = 300

# Maximum substates held in the cache, one entry each (its head version). Beyond this the oldest
# are evicted. (default: 100000)
#substate_cache_max_entries = 100000

# How long, in seconds, the cache serves a substate it has established does not exist. (default: 60)
#substate_cache_negative_ttl = 60

# Sidechain ID to listen on. Also identifies this chain for L1 burn-claim binding (optional, hex string)
#sidechain_id = "a1b2c3d4e5f6..."
```

### P2P Networking Configuration `[indexer.p2p]`

Settings for peer-to-peer networking with validator nodes.

```toml
[indexer.p2p]
# P2P listening port (default: 0)
#listener_port = 0

# Reachability mode: "Auto" or "Private" (default: "Auto")
#reachability_mode = "Auto"

# Enable mDNS peer discovery (default: true)
#enable_mdns = true

# Run a rendezvous server so other nodes can use this one for peer discovery.
# The rendezvous client is always active. (default: false)
#enable_rendezvous = false

# Enable relay (default: false)
#enable_relay = false
```

**Parameters:**
- `listener_port`: Port for incoming P2P connections
- `reachability_mode`: `Auto` (default) or `Private`
- `enable_mdns`: Automatic peer discovery on local networks
- `enable_rendezvous`: Act as a rendezvous server for other peers
- `enable_relay`: Relay traffic for other peers

### Peer Seeds Configuration `[p2p.seeds]`

Configuration for discovering and connecting to network peers. The indexer reads `[p2p.seeds]`, and the network override `[<network>.p2p.seeds]` when `p2p.seeds.override_from` is set (the CLI sets it to the selected network).

```toml
[p2p.seeds]
# DNS seed hosts for peer discovery (default: [])
#dns_seeds = ["seeds.esmeralda.tari.com"]

# Manual peer specifications (default: [])
#peer_seeds = [
#    "public_key::/ip4/1.2.3.4/tcp/18189",
#    "public_key::/dns/peer.example.com/tcp/18189"
#]
```

### Network-Specific Peer Seeds

Different networks require different peer seeds:

```toml
[esmeralda.p2p.seeds]
dns_seeds = ["seeds.esmeralda.tari.com"]
peer_seeds = [
    "a1b2c3...d4e5f6::/ip4/192.168.1.100/tcp/18189"
]

[stagenet.p2p.seeds]
dns_seeds = ["seeds.stagenet.tari.com"]
peer_seeds = []
```

### Epoch Oracle Configuration `[epoch_oracle]`

Configures how the indexer determines network epochs and validator committees.

```toml
[epoch_oracle]
# "BaseLayer", "Configured", or "Hybrid" (default: "BaseLayer").
# On MainNet this value is ignored: MainNet always runs the Hybrid oracle.
#oracle = "BaseLayer"
```

#### Base Layer Oracle `[epoch_oracle.base_layer]`

Monitors the Tari base layer for epoch information.

```toml
[epoch_oracle.base_layer]
# Minotari base node gRPC URL. When unset, the indexer uses http://127.0.0.1 and the
# network's base-node gRPC port (18142 on esmeralda).
#base_node_grpc_url = "http://127.0.0.1:18142"

# Scanning interval in seconds (default: 8)
#scanning_interval = 8
```

#### Configured Oracle `[epoch_oracle.configured]`

Uses a predefined configuration file for epoch information.

```toml
[epoch_oracle.configured]
# Path to epoch configuration file
#config_file = "epoch_oracle_config.json"
```

**Example epoch configuration file:**
```json
{
  "epoch_time": 120,
  "initial_epoch": 1,
  "validators": [
    {
      "public_key": "04efea68c9bff8f9c25fe89310abf91955e33344f4b77bb0d31ea80a80128e67",
      "claim_key": "04efea68c9bff8f9c25fe89310abf91955e33344f4b77bb0d31ea80a80128e67",
      "shard_group": {"start": 0, "end_inclusive": 127},
      "registration_epoch": 1
    },
    {
      "public_key": "02b1a2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1",
      "claim_key": "02b1a2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1",
      "shard_group": {"start": 128, "end_inclusive": 255},
      "registration_epoch": 1
    }
  ]
}
```

### Event Filtering Configuration

Control which network events are stored and indexed.

```toml
# Default empty filter matches ALL events
[[indexer.event_filters]]

# Example: Filter by topic
[[indexer.event_filters]]
topic = "ComponentCreated"

# Example: Filter by entity ID
[[indexer.event_filters]]
entity_id = "component_0123456789abcdef..."

# Example: Filter by substate ID
[[indexer.event_filters]]
substate_id = "resource_0123456789abcdef..."

# Example: Filter by template address
[[indexer.event_filters]]
template_address = "0123456789abcdef..."

# Example: Complex filter (all conditions must match)
[[indexer.event_filters]]
topic = "ResourceTransferred"
entity_id = "component_0123456789abcdef..."
substate_id = "resource_0123456789abcdef..."
```

**Filter Fields:**
- `topic`: Event type (e.g., "ComponentCreated", "ResourceTransferred"). `*` matches one dot-separated segment
- `entity_id`: Specific entity identifier
- `substate_id`: Specific substate identifier
- `template_address`: Smart contract template address
- `resource_address`: Resource address (`resource_<hex>`). Matches `std.resource.*` events, which carry the resource as their substate id

**Filter Logic:**
- Multiple `[[indexer.event_filters]]` sections are combined with OR logic
- Fields within a single filter are combined with AND logic
- An empty filter matches all events
- If no filters are specified, all events are indexed

### Rate Limits `[indexer.rate_limits]`

Per-IP token-bucket limits for the REST API. On by default. Each `*_rate` is a `(capacity, window)` pair: `capacity` is the burst size, and the bucket refills at `capacity / window` tokens per second. A request consumes one token. The default window is 10 seconds.

```toml
[indexer.rate_limits]
#enabled = true

# POST /transactions – 50 requests / 10s burst (5/s sustained)
#transactions_submit_rate = { capacity = 50.0, window = { secs = 10, nanos = 0 } }

# POST /transactions/dry-run – 30 requests / 10s burst (3/s sustained)
#transactions_dry_run_submit_rate = { capacity = 30.0, window = { secs = 10, nanos = 0 } }

# /substates/* – 300 requests / 10s burst (30/s sustained)
#substates_rate = { capacity = 300.0, window = { secs = 10, nanos = 0 } }

# /utxos/* – 300 requests / 10s burst (30/s sustained)
#utxos_fetch_rate = { capacity = 300.0, window = { secs = 10, nanos = 0 } }

# GET /transactions/* read endpoints – 200 requests / 10s burst (20/s sustained)
#transactions_rate = { capacity = 200.0, window = { secs = 10, nanos = 0 } }

# GET /epoch-checkpoints/* – 20 requests / 10s burst (2/s sustained)
#epoch_checkpoints_rate = { capacity = 20.0, window = { secs = 10, nanos = 0 } }

# GET /resources/*, /templates/* and /validators – 200 requests / 10s burst (20/s sustained)
#reads_rate = { capacity = 200.0, window = { secs = 10, nanos = 0 } }

# Maximum concurrent SSE connections per IP (default: 10)
#sse_max_connections_per_ip = 10

# Key limits on the last X-Forwarded-For entry, else X-Real-IP (default: false).
# Only enable behind a reverse proxy that appends to X-Forwarded-For.
#trust_proxy_headers = false

# Key limits on CF-Connecting-IP, ahead of X-Forwarded-For (default: false).
# Only enable when Cloudflare is in front of the indexer.
#trust_cf_connecting_ip = false
```

## Environment Variables

Configuration values can be overridden using environment variables:

```bash
# Network selection
export TARI_NETWORK=esmeralda

# Parent of the per-network data directory ($TARI_BASE_DIR/<network>; default parent ~/.tari/ootle)
export TARI_BASE_DIR=/opt/tari

# Config-file keys: TARI_ plus the dotted key, with __ for each dot.
export TARI_INDEXER__API_LISTEN_ADDRESS=0.0.0.0:18300
export TARI_INDEXER__METRICS_LISTEN_ADDRESS=127.0.0.1:18302

# These two are command-line flags, so the environment name uses a single underscore.
export TARI_INDEXER_WEB_UI_PUBLIC_API_URL=https://api.myindexer.com
export TARI_INDEXER_MINOTARI_NODE_GRPC_URL=http://localhost:18142
```

## Command Line Overrides

Use the `-p` flag to override individual configuration properties:

```bash
# Override API address
./tari_indexer -p indexer.api_listen_address=0.0.0.0:8080

# Override multiple properties  
./tari_indexer \
    -p indexer.api_listen_address=0.0.0.0:8080 \
    --epoch-oracle-minotari-node-grpc-url http://localhost:18142
```

## Common Configuration Patterns

### Development Environment

```toml
network = "localnet"

[indexer]
api_listen_address = "127.0.0.1:18300"
web_ui_address = "127.0.0.1:15000"
state_scanning_interval = 30

[indexer.p2p]
enable_mdns = true
listener_port = 18189

[epoch_oracle]
oracle = "Configured"

[epoch_oracle.configured]
config_file = "dev_epoch_config.json"

# Store all events for development
[[indexer.event_filters]]
```

### Production Environment

```toml
network = "esmeralda"

[indexer]
api_listen_address = "0.0.0.0:18300"
metrics_listen_address = "127.0.0.1:18302"
web_ui_address = "0.0.0.0:15000"
web_ui_public_api_url = "https://indexer.example.com:18300"
state_scanning_interval = 60

[indexer.p2p]
reachability_mode = "Auto"
enable_mdns = false
listener_port = 18189

[epoch_oracle]
oracle = "BaseLayer"

[epoch_oracle.base_layer]
base_node_grpc_url = "http://basenode.internal:18142"
scanning_interval = 10

[esmeralda.p2p.seeds]
dns_seeds = ["seeds.esmeralda.tari.com"]

# Filter for specific events to reduce storage
[[indexer.event_filters]]
topic = "ComponentCreated"

[[indexer.event_filters]]
topic = "ResourceTransferred"
```

### High Performance Configuration

```toml
network = "esmeralda"

[indexer]
state_scanning_interval = 10

data_dir = "/fast-ssd/tari-indexer"

[indexer.p2p]
listener_port = 18189

# High-performance base node connection
[epoch_oracle.base_layer]
base_node_grpc_url = "http://fast-basenode:18142"
scanning_interval = 2

# Store only essential events
[[indexer.event_filters]]
topic = "ComponentCreated"

[[indexer.event_filters]]
topic = "ResourceTransferred"

[[indexer.event_filters]]
topic = "TransactionResult"
```

### Privacy-Focused Configuration

```toml
network = "esmeralda"

[indexer]
# Bind to localhost only
api_listen_address = "127.0.0.1:18300"
web_ui_address = "127.0.0.1:15000"

[indexer.p2p]
reachability_mode = "Private"
enable_mdns = false

[epoch_oracle.base_layer]
base_node_grpc_url = "http://127.0.0.1:18142"
```

## Troubleshooting Configuration

### Validation

The indexer validates configuration on startup. Common issues:

**Invalid Network:**
```
Error: Invalid network 'invalid-network'
Valid networks: localnet, esmeralda, stagenet, nextnet
```

**Port Conflicts:**
```
Error: Failed to bind to 127.0.0.1:18300: Address already in use
```

**Missing Files:**
```
Error: Epoch oracle config file not found: epoch_config.json
```

### Debugging Configuration

Enable debug logging to see active configuration:

```bash
# Set debug level logging
export RUST_LOG=tari_indexer=debug

# Run indexer to see loaded configuration
./tari_indexer
```

### Configuration Precedence

Configuration values are applied in this order (highest precedence first):

1. Command line arguments (`-p`, `--api-listen-address`, etc.)
2. Environment variables (`TARI_NETWORK`, `TARI_BASE_DIR`, `TARI_INDEXER__*` for config keys, and the flag-backed `TARI_INDEXER_WEB_UI_PUBLIC_API_URL` / `TARI_INDEXER_MINOTARI_NODE_GRPC_URL`)  
3. Configuration file values
4. Network-specific defaults (`[esmeralda.indexer]`)
5. Global defaults (`[indexer]`)
6. Built-in defaults

### Backup and Recovery

**Backup Configuration:**
```bash
# Backup configuration directory
cp -r ~/.tari/config ~/.tari/config.backup

# Or backup specific file
cp ~/.tari/config/config.toml ~/.tari/config.toml.backup
```

**Recovery:**
```bash
# Restore from backup
cp ~/.tari/config.toml.backup ~/.tari/config/config.toml

# Or regenerate default configuration
rm ~/.tari/config/config.toml
./tari_indexer  # Will create default config
```