# Helium

Minimal Rust implementation of Cosmos SDK BaseApp

## Installation

```bash
cargo build --release
```

## CLI Usage

The `helium` binary provides a command-line interface for managing and running a Helium blockchain node.

### Commands

#### Initialize Node

Initialize a new node with configuration and data directory:

```bash
# Initialize with custom home directory
helium init --chain-id helium-testnet-1 --home ~/.helium

# Initialize with genesis file
helium init --chain-id helium-testnet-1 --genesis ./genesis.json
```

#### Start Node

Start the node and connect to the network:

```bash
# Start with default configuration
helium start

# Start with custom home directory
helium start --home ~/.helium

# Start with custom config and log level
helium start --config ./custom-config.toml --log-level debug
```

#### Version

Display version information:

```bash
helium version
```

#### Genesis Utilities

Validate and export genesis files:

```bash
# Validate a genesis file
helium genesis validate ./genesis.json

# Export current genesis state
helium genesis export --home ~/.helium --output genesis-export.json
```

#### Key Management

Manage keys for the node:

```bash
# Add a new key
helium keys add mykey

# Recover key from mnemonic
helium keys add mykey --recover

# List all keys
helium keys list

# Show key details
helium keys show mykey

# Delete a key
helium keys delete mykey
```

#### Configuration Management

View and validate configuration:

```bash
# Show current configuration
helium config show --home ~/.helium

# Validate a configuration file
helium config validate ./config.toml
```

### Configuration

The main configuration file (`app.toml`) includes:

```toml
# Application Configuration
chain_id = "helium-testnet-1"

[app]
minimum_gas_prices = "0.025uhelium"
pruning = "default"
halt_height = 0

[api]
enable = true
address = "tcp://0.0.0.0:1317"
max_open_connections = 1000
rpc_read_timeout = 10
rpc_write_timeout = 0

[grpc]
enable = true
address = "0.0.0.0:9090"

[wasm]
modules_dir = "./wasm_modules"
cache_size = 100
memory_limit = "512MB"
```

### Environment Variables

- `RUST_LOG`: Set log level (trace, debug, info, warn, error)

### Examples

#### Quick Start

```bash
# Initialize a new node
helium init --chain-id helium-testnet-1

# Start the node
helium start
```

#### Development Setup

```bash
# Initialize with debug logging
RUST_LOG=debug helium init --chain-id helium-dev

# Start with custom configuration
helium start --config ./dev-config.toml --log-level trace
```