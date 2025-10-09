# Tari Indexer

The Tari Indexer is a service that indexes and tracks substates, transactions, and events on the Tari DAN (Digital Assets Network). It provides REST and GraphQL APIs for querying blockchain data and includes a web-based user interface for exploring the network.

## Overview

The indexer continuously scans the DAN layer for new blocks, transactions, and substates, storing this information in a local SQLite database. It provides multiple interfaces for accessing this data:

- **REST API**: RESTful endpoints for programmatic access
- **GraphQL API**: Flexible query interface for complex data retrieval  
- **Web UI**: Browser-based interface for exploring network data
- **UTXO Streaming**: Real-time streaming of UTXO updates

## Features

- Real-time blockchain data indexing
- REST and GraphQL APIs
- Web-based user interface
- UTXO streaming capabilities
- P2P networking for validator node communication
- Configurable scanning intervals
- Template and NFT tracking
- Transaction and event monitoring

## Building

```bash
cargo build --release
```

### Features

- `web_ui` (default): Includes the web-based user interface
- Without web UI: `cargo build --release --no-default-features`

## Configuration

The indexer uses configuration files and environment variables. Key configuration sections:

- **API Endpoints**: Configure REST and GraphQL server addresses
- **P2P Networking**: Set peer seeds, ports, and reachability modes
- **Database**: SQLite storage configuration
- **Scanning**: Block scanning intervals and filters
- **Web UI**: Public API URLs for the web interface

## Running

### Basic Usage

```bash
./target/release/tari_indexer
```

### Command Line Options

- `-r, --api-listen-address <ADDR>`: Bind address for REST API server
- `-g, --node-grpc <URL>`: Minotari node gRPC URL for epoch oracle
- `-s, --peer-seeds <SEEDS>`: P2P peer seeds (space-separated)
- `--listener-port <PORT>`: P2P listening port
- `-i, --scanning-interval <SECONDS>`: Block scanning interval
- `--web-ui-public-api-url <URL>`: Public API URL for web UI
- `--web-ui-public-graphql-url <URL>`: Public GraphQL URL for web UI
- `--reachability <MODE>`: P2P reachability mode (reachable/unreachable)
- `--disable-mdns`: Disable mDNS peer discovery

## APIs

### REST API

When running, the REST API is available at the configured address (default: `http://localhost:8080`). Key endpoints include:

- `/templates` - Template information
- `/transactions` - Transaction data
- `/substates` - Substate queries
- `/nfts` - NFT tracking
- `/utxos/stream` - UTXO streaming
- `/network` - Network information

Swagger documentation is available at `/swagger-ui/` when the server is running.

### GraphQL API

The GraphQL endpoint provides flexible querying capabilities for:
- Transactions and their details
- Substates and their changes
- Events and logs
- Template information

### Web UI

The web interface (when enabled) provides:
- Network overview and statistics
- Transaction browsing and search
- Substate exploration
- Event monitoring
- Resource and NFT tracking

## Database

The indexer uses SQLite for local storage, automatically creating and migrating the database schema on startup. The database location is configurable through the data directory setting.

## Development

### Web UI Development

The web UI is a React application located in the `web_ui/` directory:

```bash
cd web_ui/
npm install
npm run dev  # Development server
npm run build  # Production build
```

### Testing

Run the test suite:

```bash
cargo test
```

## Architecture

The indexer consists of several key components:

- **Block Scanner**: Continuously scans for new blocks and events
- **Storage Layer**: SQLite-based data persistence
- **REST Server**: HTTP API endpoints
- **GraphQL Server**: Flexible query interface
- **P2P Networking**: Communication with validator nodes
- **Event Manager**: Real-time event processing
- **Web UI Server**: Static file serving for the web interface

## License

Copyright 2023, The Tari Project. Licensed under BSD 3-Clause.