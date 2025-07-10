# CometBFT Integration Guide

This guide explains how to set up and run Helium with CometBFT consensus engine.

## Overview

Helium integrates with CometBFT (formerly Tendermint) through the ABCI++ protocol. This allows Helium to leverage CometBFT's Byzantine Fault Tolerant consensus while maintaining the application logic in Rust with WASI modules.

## Architecture

```
┌─────────────────┐     ABCI++      ┌─────────────────┐
│                 │ ◄─────────────► │                 │
│    CometBFT     │                 │     Helium      │
│  (Consensus)    │                 │  (Application)  │
│                 │                 │                 │
└─────────────────┘                 └─────────────────┘
     Port 26656/7                        Port 26658
```

- **CometBFT**: Handles consensus, P2P networking, and block production
- **Helium**: Processes transactions and manages application state
- **ABCI++**: Protocol for communication between consensus and application

## Quick Start

### Prerequisites

- Docker and Docker Compose installed
- Git
- Bash shell

### 1. Clone and Setup

```bash
git clone https://github.com/mconcat/helium.git
cd helium
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
- Set up Helium configuration

### 3. Start Testnet

```bash
./scripts/start-testnet.sh
```

This will launch both CometBFT and Helium services using Docker Compose.

### 4. Verify Services

Check that services are running:

```bash
# Check CometBFT status
curl http://localhost:26657/status

# Check Helium health
curl http://localhost:1317/health

# Check if ready
curl http://localhost:1317/ready
```

## Service Endpoints

| Service | Endpoint | Description |
|---------|----------|-------------|
| CometBFT RPC | http://localhost:26657 | Consensus engine RPC |
| CometBFT P2P | tcp://localhost:26656 | P2P communication |
| Helium ABCI | tcp://localhost:26658 | ABCI protocol |
| Helium gRPC | http://localhost:9090 | gRPC API |
| Helium REST | http://localhost:1317 | REST API & Health |

## Configuration

### Environment Variables

Key environment variables in `.env`:

```bash
CHAIN_ID=helium-testnet          # Chain identifier
MONIKER=helium-node-0           # Node moniker
RUST_LOG=info                   # Rust log level
COMETBFT_LOG_LEVEL=info        # CometBFT log level
```

### CometBFT Configuration

Located at `testnet/node0/config/config.toml`:

```toml
proxy_app = "tcp://helium:26658"
moniker = "helium-node-0"

[consensus]
timeout_propose = "3s"
timeout_prevote = "1s"
timeout_precommit = "1s"
timeout_commit = "5s"
```

### Helium Configuration

Located at `testnet/helium/config/config.toml`:

```toml
listen_address = "tcp://0.0.0.0:26658"
grpc_address = "0.0.0.0:9090"
chain_id = "helium-testnet"
```

## Operations

### View Logs

```bash
# All services
docker-compose logs -f

# Just CometBFT
docker-compose logs -f cometbft

# Just Helium
docker-compose logs -f helium
```

### Stop Services

```bash
docker-compose down
```

### Reset Testnet

To reset blockchain data while preserving configuration:

```bash
./scripts/reset-testnet.sh
```

### Complete Cleanup

To remove everything including configuration:

```bash
docker-compose down -v
rm -rf testnet/
```

## Development

### Building from Source

```bash
# Build Docker image
docker-compose build

# Build locally
cargo build --release
```

### Running Without Docker

1. Install CometBFT binary
2. Build Helium: `cargo build --release`
3. Initialize: `./scripts/init-testnet.sh`
4. Start Helium: `./target/release/helium-server start`
5. Start CometBFT: `cometbft node --home ./testnet/node0`

## Troubleshooting

### Connection Issues

If CometBFT cannot connect to Helium:

1. Check Helium is running: `docker-compose ps`
2. Verify ABCI port: `nc -zv localhost 26658`
3. Check logs: `docker-compose logs helium`

### Consensus Issues

If blocks are not being produced:

1. Check CometBFT status: `curl http://localhost:26657/status`
2. Verify genesis file: `cat testnet/node0/config/genesis.json`
3. Check validator key: `cat testnet/node0/config/priv_validator_key.json`

### Performance Issues

1. Monitor resource usage: `docker stats`
2. Adjust CometBFT timeouts in config
3. Check Helium logs for slow operations

## Multi-Node Setup

For a 4-node testnet, use the multi-node configuration:

```bash
# Initialize multi-node testnet
./scripts/init-multi-testnet.sh

# Start multi-node testnet
docker-compose -f docker-compose.multi.yml up
```

## Security Considerations

1. **Private Keys**: Keep validator keys secure
2. **Firewall**: Only expose necessary ports
3. **TLS**: Enable TLS for production deployments
4. **Monitoring**: Set up monitoring and alerting

## Monitoring

### Metrics

Helium exposes metrics at:
- Health: `http://localhost:1317/health`
- Ready: `http://localhost:1317/ready`
- Swagger: `http://localhost:1317/swagger`

### Prometheus Integration

Add to `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'helium'
    static_configs:
      - targets: ['localhost:1317']
```

## Advanced Configuration

### Custom Genesis State

Edit `testnet/helium/genesis.json` before starting:

```json
{
  "chain_id": "helium-testnet",
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

WASI modules are loaded from `/usr/local/lib/helium/wasi-modules/` in the container.

## References

- [CometBFT Documentation](https://docs.cometbft.com/)
- [ABCI++ Specification](https://docs.cometbft.com/v0.38/spec/abci/)
- [Helium Architecture](../PLAN.md)