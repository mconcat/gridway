# CometBFT Integration Guide

This guide explains how to set up and run Gridway with CometBFT consensus engine.

## Overview

Gridway integrates with CometBFT (formerly Tendermint) through the ABCI++ protocol. This allows Gridway to leverage CometBFT's Byzantine Fault Tolerant consensus while maintaining the application logic in Rust with WASI modules.

## Architecture

```
┌─────────────────┐     ABCI++      ┌─────────────────┐
│                 │ ◄─────────────► │                 │
│    CometBFT     │                 │     Gridway      │
│  (Consensus)    │                 │  (Application)  │
│                 │                 │                 │
└─────────────────┘                 └─────────────────┘
     Port 26656/7                        Port 26658
```

- **CometBFT**: Handles consensus, P2P networking, and block production
- **Gridway**: Processes transactions and manages application state
- **ABCI++**: Protocol for communication between consensus and application

## Quick Start

### Prerequisites

- Docker and Docker Compose installed
- Git
- Bash shell

### 1. Clone and Setup

```bash
git clone https://github.com/mconcat/gridway.git
cd gridway
cp .env.example .env
```

### 2. Initialize Testnet

```bash
./scripts/init-testnet.sh
```

This script will:
- Create testnet directories
- Initialize CometBFT configuration
- Generate validator keys
- Create genesis file
- Set up Gridway configuration

### 3. Start Testnet

```bash
./scripts/start-testnet.sh
```

This will launch both CometBFT and Gridway services using Docker Compose.

### 4. Verify Services

Check that services are running:

```bash
# Check CometBFT status
curl http://localhost:26657/status

# Check Gridway health
curl http://localhost:1317/health

# Check if ready
curl http://localhost:1317/ready
```

## Service Endpoints

| Service | Endpoint | Description |
|---------|----------|-------------|
| CometBFT RPC | http://localhost:26657 | Consensus engine RPC |
| CometBFT P2P | tcp://localhost:26656 | P2P communication |
| Gridway ABCI | tcp://localhost:26658 | ABCI protocol |
| Gridway gRPC | http://localhost:9090 | gRPC API |
| Gridway REST | http://localhost:1317 | REST API & Health |

## Configuration

### Environment Variables

Key environment variables in `.env`:

```bash
CHAIN_ID=gridway-testnet          # Chain identifier
MONIKER=gridway-node-0           # Node moniker
RUST_LOG=info                   # Rust log level
COMETBFT_LOG_LEVEL=info        # CometBFT log level
```

### CometBFT Configuration

Located at `testnet/node0/config/config.toml`:

```toml
proxy_app = "tcp://gridway:26658"
moniker = "gridway-node-0"

[consensus]
timeout_propose = "3s"
timeout_prevote = "1s"
timeout_precommit = "1s"
timeout_commit = "5s"
```

### Gridway Configuration

Located at `testnet/gridway/config/config.toml`:

```toml
listen_address = "tcp://0.0.0.0:26658"
grpc_address = "0.0.0.0:9090"
chain_id = "gridway-testnet"
```

## Operations

### View Logs

```bash
# All services
docker compose logs -f

# Just CometBFT
docker compose logs -f cometbft

# Just Gridway
docker compose logs -f gridway
```

### Stop Services

```bash
docker compose down
```

### Reset Testnet

To reset blockchain data while preserving configuration:

```bash
./scripts/reset-testnet.sh
```

### Complete Cleanup

To remove everything including configuration:

```bash
docker compose down -v
rm -rf testnet/
```

## Development

### Building from Source

```bash
# Build Docker image
docker compose build

# Build locally
cargo build --release
```

### Running Without Docker

1. Install CometBFT binary
2. Build Gridway: `cargo build --release`
3. Initialize: `./scripts/init-testnet.sh`
4. Start Gridway: `./target/release/gridway-server start`
5. Start CometBFT: `cometbft node --home ./testnet/node0`

## Troubleshooting

### Connection Issues

If CometBFT cannot connect to Gridway:

1. Check Gridway is running: `docker compose ps`
2. Verify ABCI port: `nc -zv localhost 26658`
3. Check logs: `docker compose logs gridway`

### Consensus Issues

If blocks are not being produced:

1. Check CometBFT status: `curl http://localhost:26657/status`
2. Verify genesis file: `cat testnet/node0/config/genesis.json`
3. Check validator key: `cat testnet/node0/config/priv_validator_key.json`

### Performance Issues

1. Monitor resource usage: `docker stats`
2. Adjust CometBFT timeouts in config
3. Check Gridway logs for slow operations

## Multi-Node Setup

For a 4-node testnet, use the multi-node configuration:

```bash
# Initialize multi-node testnet
./scripts/init-multi-testnet.sh

# Start multi-node testnet
docker compose -f docker compose.multi.yml up
```

## Security Considerations

1. **Private Keys**: Keep validator keys secure
2. **Firewall**: Only expose necessary ports
3. **TLS**: Enable TLS for production deployments
4. **Monitoring**: Set up monitoring and alerting

## Monitoring

### Metrics

Gridway exposes metrics at:
- Health: `http://localhost:1317/health`
- Ready: `http://localhost:1317/ready`
- Swagger: `http://localhost:1317/swagger`

### Prometheus Integration

Add to `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'gridway'
    static_configs:
      - targets: ['localhost:1317']
```

## Advanced Configuration

### Custom Genesis State

Edit `testnet/gridway/genesis.json` before starting:

```json
{
  "chain_id": "gridway-testnet",
  "app_state": {
    "auth": {
      "accounts": []
    },
    "bank": {
      "balances": []
    }
  }
}
```

### WASI Module Configuration

WASI modules are loaded from `/usr/local/lib/gridway/wasi-modules/` in the container.

## References

- [CometBFT Documentation](https://docs.cometbft.com/)
- [ABCI++ Specification](https://docs.cometbft.com/v0.38/spec/abci/)
- [Gridway Architecture](../PLAN.md)