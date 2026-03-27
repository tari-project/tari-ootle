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

# Base directory for all Tari data (default: ~/.tari)
#base_path = "/path/to/tari/data"
```

**Parameters:**
- `override_from`: Network name to inherit configuration from (e.g., "esmeralda", "stagenet", "localnet")
- `base_path`: Root directory for all Tari application data storage

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

# GraphQL server address (default: "127.0.0.1:18301")
#graphql_address = "127.0.0.1:18301"

# Web UI server address (default: "127.0.0.1:15000")
#web_ui_address = "127.0.0.1:15000"

# Public API URL for web UI (optional, defaults to api_listen_address)
#web_ui_public_api_url = "https://myindexer.example.com:18300"

# Public GraphQL URL for web UI (optional, defaults to graphql_address)
#web_ui_public_graphql_url = "https://myindexer.example.com:18301"

# Block scanning interval in seconds (default: 10)
#block_scanning_interval = 10

# State scanning interval in seconds (default: 60)
#state_scanning_interval = 60

# Sidechain ID to listen on (optional, hex string)
#sidechain_id = "a1b2c3d4e5f6..."

# Templates sidechain ID (optional, hex string)
#templates_sidechain_id = "a1b2c3d4e5f6..."

# Burnt UTXO sidechain ID (optional, hex string)
#burnt_utxo_sidechain_id = "a1b2c3d4e5f6..."
```

### P2P Networking Configuration `[indexer.p2p]`

Settings for peer-to-peer networking with validator nodes.

```toml
[indexer.p2p]
# P2P listening port (default: varies by network)
#listener_port = 18189

# Reachability mode: "reachable" or "unreachable" (default: "reachable")
#reachability_mode = "reachable"

# Enable mDNS peer discovery (default: true)
#enable_mdns = true

# Enable rendezvous server functionality (default: false)
#enable_rendezvous = false
```

**Parameters:**
- `listener_port`: Port for incoming P2P connections
- `reachability_mode`: Whether the node can accept incoming connections
- `enable_mdns`: Automatic peer discovery on local networks
- `enable_rendezvous`: Act as a rendezvous server for other peers

### Peer Seeds Configuration `[peer_seeds]`

Configuration for discovering and connecting to network peers.

```toml
[peer_seeds]
# DNS seed hosts for peer discovery (default: [])
#dns_seeds = ["seeds.esmeralda.tari.com"]

# Manual peer specifications (default: [])
#peer_seeds = [
#    "public_key::/ip4/1.2.3.4/tcp/18189",
#    "public_key::/dns/peer.example.com/tcp/18189"
#]

# DNS server for seed resolution (default: "1.1.1.1:853/cloudflare-dns.com")
#dns_seeds_name_server = "1.1.1.1:853/cloudflare-dns.com"

# Require DNSSEC validation for DNS seeds (default: false)
#dns_seeds_use_dnssec = false

# Rendezvous server for peer discovery (optional)
#rendezvous_server = "public_key::/ip4/1.2.3.4/tcp/18189"
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
# Oracle type: "BaseLayer", "Configured", or "Hybrid" (default: "BaseLayer")
#oracle_type = "BaseLayer"
```

#### Base Layer Oracle `[epoch_oracle.base_layer]`

Monitors the Tari base layer for epoch information.

```toml
[epoch_oracle.base_layer]
# Minotari base node gRPC URL (default: network-specific)
#base_node_grpc_url = "http://localhost:18142"

# Scanning interval in seconds (default: 5)
#scanning_interval = 5
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
- `topic`: Event type (e.g., "ComponentCreated", "ResourceTransferred")
- `entity_id`: Specific entity identifier
- `substate_id`: Specific substate identifier  
- `template_address`: Smart contract template address

**Filter Logic:**
- Multiple `[[indexer.event_filters]]` sections are combined with OR logic
- Fields within a single filter are combined with AND logic
- An empty filter matches all events
- If no filters are specified, all events are indexed

### Template Configuration `[indexer.templates]`

Settings for smart contract template management.

```toml
[indexer.templates]
# Template storage directory (default: "templates")
#template_dir = "templates"

# Enable automatic template downloading (default: true)
#auto_download = true

# Maximum template size in bytes (default: 1048576 = 1MB)
#max_template_size = 1048576
```

### Auto Update Configuration `[auto_update]`

Automatic software update settings.

```toml
[auto_update]
# Check interval in seconds (0 = disabled, default: 300)
#check_interval = 300

# DNS server for update queries
#name_server = "1.1.1.1:53/cloudflare.net"

# Update information sources
#update_uris = []

# Enable DNSSEC validation
#use_dnssec = false

# Base URL for downloading updates
#download_base_url = ""

# Hash and signature URLs for verification
#hashes_url = "https://releases.tari.com/hashes.txt"
#hashes_sig_url = "https://releases.tari.com/hashes.txt.sig"
```

### Metrics Configuration `[metrics]`

Monitoring and metrics collection settings.

```toml
[metrics]
# Metrics server bind address (default: disabled)
#server_bind_address = "127.0.0.1:5577"

# Prometheus push gateway endpoint (optional)
#push_endpoint = "http://localhost:9091/metrics/job/tari-indexer"
```

## Environment Variables

Configuration values can be overridden using environment variables:

```bash
# Network selection
export TARI_NETWORK=esmeralda

# Base data directory  
export TARI_BASE_DIR=/opt/tari

# Indexer-specific overrides
export TARI_INDEXER_API_LISTEN_ADDRESS=0.0.0.0:18300
export TARI_INDEXER_WEB_UI_PUBLIC_API_URL=https://api.myindexer.com
export TARI_INDEXER_WEB_UI_PUBLIC_GRAPHQL_URL=https://graphql.myindexer.com
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
    -p indexer.block_scanning_interval=5 \
    -p epoch_oracle.base_layer.base_node_grpc_url=http://localhost:18142
```

## Common Configuration Patterns

### Development Environment

```toml
network = "localnet"

[indexer]
api_listen_address = "127.0.0.1:18300"
web_ui_address = "127.0.0.1:15000"
block_scanning_interval = 5  # Faster scanning for development
state_scanning_interval = 30

[indexer.p2p]
enable_mdns = true
listener_port = 18189

[epoch_oracle]
oracle_type = "Configured"

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
web_ui_address = "0.0.0.0:15000"
web_ui_public_api_url = "https://indexer.example.com:18300"
web_ui_public_graphql_url = "https://indexer.example.com:18301"
block_scanning_interval = 10
state_scanning_interval = 60

[indexer.p2p]
reachability_mode = "reachable"
enable_mdns = false
listener_port = 18189

[epoch_oracle]
oracle_type = "BaseLayer"

[epoch_oracle.base_layer]
base_node_grpc_url = "http://basenode.internal:18142"
scanning_interval = 10

[esmeralda.p2p.seeds]
dns_seeds = ["seeds.esmeralda.tari.com"]

[metrics]
server_bind_address = "127.0.0.1:5577"

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
# Fast scanning for real-time applications
block_scanning_interval = 2
state_scanning_interval = 10

# Large cache for better performance
data_dir = "/fast-ssd/tari-indexer"

[indexer.p2p]
# Multiple connections for redundancy
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
# Unreachable mode for privacy
reachability_mode = "unreachable"
enable_mdns = false

# Use Tor for base node connection (if available)
[epoch_oracle.base_layer]
base_node_grpc_url = "socks5://127.0.0.1:9050/basenode.onion:18142"

# Disable metrics collection
[metrics]
# server_bind_address = disabled
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
2. Environment variables (`TARI_INDEXER_*`)  
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