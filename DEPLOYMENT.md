# Gridway Deployment Guide

[![CI](https://github.com/mconcat/gridway/actions/workflows/ci.yml/badge.svg)](https://github.com/mconcat/gridway/actions/workflows/ci.yml)
[![CometBFT Integration](https://github.com/mconcat/gridway/actions/workflows/cometbft-integration.yml/badge.svg)](https://github.com/mconcat/gridway/actions/workflows/cometbft-integration.yml)
[![Docker](https://github.com/mconcat/gridway/actions/workflows/docker.yml/badge.svg)](https://github.com/mconcat/gridway/actions/workflows/docker.yml)

This guide covers deploying and operating Gridway blockchain nodes with CometBFT consensus engine, including single-node testnets and multi-validator networks.

## 🚀 Quick Start

```bash
# 1. Clone the repository
git clone https://github.com/mconcat/gridway.git
cd gridway

# 2. Copy environment configuration
cp .env.example .env

# 3. Initialize single-node testnet
./scripts/init-testnet.sh

# 4. Start the testnet
./scripts/start-testnet.sh
```

## 📁 Project Structure

```
.
├── docker compose.yml          # Single-node Docker configuration
├── docker compose.multi.yml    # Multi-node Docker configuration
├── Dockerfile                  # Gridway application image
├── .env.example               # Environment variables template
├── scripts/
│   ├── init-testnet.sh        # Initialize single-node testnet
│   ├── start-testnet.sh       # Start testnet services
│   ├── reset-testnet.sh       # Reset blockchain data
│   └── init-multi-testnet.sh  # Initialize 4-node testnet
└── docs/
    └── cometbft-integration.md # Detailed documentation
```

## 🛠️ Key Features

- **ABCI++ Integration**: Full support for CometBFT v0.38.0 with ABCI++ protocol
- **Health Monitoring**: Built-in health check endpoints at `/health` and `/ready`
- **Connection Resilience**: Automatic reconnection with exponential backoff
- **Multi-Node Support**: Easy setup for 4-node validator networks
- **Docker-Based**: Containerized deployment for consistency

## 📊 Service Endpoints

| Service | Default Port | Description |
|---------|-------------|-------------|
| CometBFT RPC | 26657 | Consensus engine RPC |
| CometBFT P2P | 26656 | P2P network communication |
| Gridway ABCI | 26658 | ABCI protocol interface |
| Gridway gRPC | 9090 | gRPC API endpoint |
| Gridway REST | 1317 | REST API & Health endpoints |

## 🔧 Common Operations

### View Logs
```bash
docker compose logs -f
```

### Check Status
```bash
# CometBFT status
curl http://localhost:26657/status

# Application health
curl http://localhost:1317/health
```

### Reset Testnet
```bash
./scripts/reset-testnet.sh
```

### Stop Services
```bash
docker compose down
```

## 🌐 Multi-Node Testnet

For a 4-node validator network:

```bash
# Initialize multi-node setup
./scripts/init-multi-testnet.sh

# Start all nodes
docker compose -f docker compose.multi.yml up
```

## 🔍 Troubleshooting

### Connection Issues
- Verify Gridway is running: `docker compose ps`
- Check ABCI port: `nc -zv localhost 26658`
- Review logs: `docker compose logs gridway`

### Build Issues
- Ensure Docker is running
- Check available disk space
- Verify network connectivity

## 📚 Documentation

- [Detailed Integration Guide](docs/cometbft-integration.md)
- [CometBFT Documentation](https://docs.cometbft.com/)
- [ABCI++ Specification](https://docs.cometbft.com/v0.38/spec/abci/)

## 🤝 Contributing

See the main [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## 📄 License

This project is licensed under AGPL-3.0. See [LICENSE](../LICENSE) for details.