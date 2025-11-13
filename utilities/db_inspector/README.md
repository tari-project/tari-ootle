# DB Inspector

A web-based tool for inspecting and browsing RocksDB databases used by Tari validator nodes. DB Inspector provides a read-only interface to explore database contents, column families, blocks, state transitions, and other internal data structures.

## Features

- Browse multiple RocksDB databases simultaneously
- View all column families in a database
- Inspect blocks, block diffs, and state transitions
- Search and filter database entries
- Group databases for better organization
- Web-based UI with dark/light theme support
- Read-only access using RocksDB secondary instances (safe to use with running nodes)

## Quick Start

### Basic Usage

1. **Run with default configuration:**
   ```bash
   cargo run --bin db_inspector
   ```

   This will:
   - Create a default configuration file at `data/db_inspector.toml`
   - Start the web server on `http://127.0.0.1:9090`
   - Open a single validator node database (VN0)

2. **Access the web interface:**
   Open your browser to `http://127.0.0.1:9090`

### Custom Configuration

1. **Copy the example configuration:**
   ```bash
   cp utilities/db_inspector/config.example.toml data/db_inspector.toml
   ```

2. **Edit the configuration** to add your database paths:
   ```toml
   [webserver]
   bind_address = "127.0.0.1:9090"

   [[dbs]]
   path = "path/to/your/rocksdb"
   secondary_path = "data/db_inspector/secondaries/vn0"
   name = "VN0"
   group = "validators"  # optional
   ```

3. **Run with custom config:**
   ```bash
   cargo run --bin db_inspector -- --config-path data/db_inspector.toml
   ```

## Configuration

### Configuration File Structure

```toml
[webserver]
bind_address = "127.0.0.1:9090"

[[dbs]]
path = "data/validator_node/rocksdb"
secondary_path = "data/db_inspector/secondaries/vn0"
name = "Validator Node 0"
group = "validators"  # optional: group databases in the UI
```

### Configuration Fields

- **webserver.bind_address**: The address and port to bind the web server to
- **dbs**: Array of database configurations
  - **path**: Path to the primary RocksDB database (read-only)
  - **secondary_path**: Path where the secondary instance will be created
  - **name**: Display name for the database in the UI
  - **group**: (Optional) Group name to organize databases in the UI

### Command Line Options

```bash
db_inspector [OPTIONS]

Options:
  --config-path <PATH>  Path to configuration file [default: data/db_inspector.toml]
  -n, --db-name <NAME>  Name of specific database to inspect (if config has multiple)
  -h, --help            Print help information
  -V, --version         Print version information
```

## How It Works

### RocksDB Secondary Instances

DB Inspector uses RocksDB's secondary instance feature to safely read from databases that may be in use by running validator nodes:

- **Primary database**: The original RocksDB used by the validator node (read-only access)
- **Secondary instance**: A separate RocksDB instance that follows the primary database
- **No interference**: The secondary instance doesn't lock or modify the primary database
- **Safe for production**: Can inspect running validator nodes without disruption

The `secondary_path` in the configuration is where the secondary instance stores its metadata. This directory will be created automatically.

## API Endpoints

The tool exposes a REST API at `/api`:

### Databases
- `GET /api/databases` - List all configured databases

### Column Families
- `GET /api/databases/{db_name}/column-families` - List all column families in a database
- `GET /api/databases/{db_name}/column-families/{cf_name}` - View entries in a column family

### Special Handlers
- `GET /api/databases/{db_name}/column-families/blocks` - View blocks with enhanced formatting
- `GET /api/databases/{db_name}/column-families/state_transitions` - View state transitions
- `GET /api/databases/{db_name}/column-families/block_diff` - View block diffs
- `GET /api/databases/{db_name}/column-families/bookkeeping` - View bookkeeping data
- `GET /api/databases/{db_name}/column-families/foreign_substate_pledges` - View foreign substate pledges

### Supported Column Families

The inspector supports viewing data from numerous column families including:

- Blocks and block indices
- State transitions and substates
- Transaction data and execution results
- Certificates (proposals, timeouts)
- Foreign proposals and parked blocks
- Substate locks and state trees
- Epoch checkpoints
- Lock conflicts
- Validator node statistics

See [server.rs:71-114](src/webserver/server.rs#L71-L114) for the complete list.

## Web UI

The web interface is built with React and Material-UI, providing:

- Database selector with grouping
- Column family browser
- Data grid with sorting and filtering
- JSON viewer for complex data structures
- Dark/light theme toggle
- Responsive design

### Building the Web UI

The web UI is located in `web_ui/` and is automatically embedded in the binary:

```bash
cd utilities/db_inspector/web_ui
npm install
npm run build
```

The built files in `web_ui/dist/` are embedded using Rust's `include_dir!` macro at compile time.

## Use Cases

### Development and Debugging
- Inspect database state during development
- Verify data storage and retrieval
- Debug consensus issues
- Examine block and transaction history

### Testing
- Validate database contents in integration tests
- Compare state across multiple validator nodes
- Inspect test swarm databases

### Operations
- Monitor validator node state
- Investigate network issues
- Audit historical data
- Troubleshoot node synchronization

## Example: Inspecting a Local Swarm

If you're running a local test swarm, you can inspect all validator nodes:

```toml
[webserver]
bind_address = "127.0.0.1:9090"

[[dbs]]
path = "data/swarm/processes/validator-node-00/localnet/data/validator_node/rocksdb"
secondary_path = "data/db_inspector/secondaries/vn0"
name = "VN0"

[[dbs]]
path = "data/swarm/processes/validator-node-01/localnet/data/validator_node/rocksdb"
secondary_path = "data/db_inspector/secondaries/vn1"
name = "VN1"

# Add more nodes as needed...
```

## Safety and Limitations

### Safe Operations
- Read-only access to databases
- Uses RocksDB secondary instances (no locks on primary DB)
- Can run alongside active validator nodes
- CORS enabled for cross-origin requests

### Limitations
- Cannot modify database contents
- May lag slightly behind the primary database
- Secondary path requires disk space for metadata
- Large databases may take time to load in the UI

## Troubleshooting

### Port Already in Use
If port 9090 is in use, the server will automatically try an OS-assigned port:
```
Failed to bind on preferred address 127.0.0.1:9090. Trying OS-assigned
Webserver listening on http://127.0.0.1:xxxxx
```

### Database Path Not Found
Ensure the `path` in your config points to a valid RocksDB directory. The path should contain the RocksDB database files (typically `.sst` files and a `CURRENT` file).

### Secondary Path Permissions
Make sure the `secondary_path` directory is writable. The tool will create this directory if it doesn't exist.

## License

Copyright 2025 The Tari Project
SPDX-License-Identifier: BSD-3-Clause
