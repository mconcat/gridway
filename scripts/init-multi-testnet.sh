#!/bin/bash
# Initialize a 4-node CometBFT testnet with Gridway application

set -e

# Load environment variables
if [ -f .env ]; then
    source .env
else
    echo "Warning: .env file not found, using defaults"
fi

# Set defaults if not provided
CHAIN_ID=${CHAIN_ID:-"gridway-testnet"}
TESTNET_DIR=${TESTNET_DIR:-"./testnet"}
NUM_NODES=4

echo "Initializing 4-node testnet..."
echo "Chain ID: ${CHAIN_ID}"
echo "Testnet directory: ${TESTNET_DIR}"

# Clean existing data
if [ -d "${TESTNET_DIR}" ]; then
    echo "Removing existing testnet directory..."
    rm -rf "${TESTNET_DIR}"
fi

# Create testnet using CometBFT testnet command
echo "Creating CometBFT testnet configuration..."
docker run --rm -v "$(pwd)/${TESTNET_DIR}":/testnet \
    cometbft/cometbft:v0.38.0 \
    testnet --v ${NUM_NODES} \
    --chain-id ${CHAIN_ID} \
    --o /testnet \
    --starting-ip-address 172.28.0.2

# Update node configurations
for i in $(seq 0 $((NUM_NODES-1))); do
    NODE_DIR="${TESTNET_DIR}/node${i}"
    CONFIG_FILE="${NODE_DIR}/config/config.toml"
    
    echo "Configuring node ${i}..."
    
    # Update proxy_app to point to corresponding Gridway instance
    sed -i.bak "s|proxy_app = \"kvstore\"|proxy_app = \"tcp://gridway-${i}:26658\"|g" "${CONFIG_FILE}"
    
    # Update RPC and P2P listen addresses
    sed -i.bak "s|laddr = \"tcp://0.0.0.0:26657\"|laddr = \"tcp://0.0.0.0:26657\"|g" "${CONFIG_FILE}"
    sed -i.bak "s|laddr = \"tcp://0.0.0.0:26656\"|laddr = \"tcp://0.0.0.0:26656\"|g" "${CONFIG_FILE}"
    
    # Enable CORS for RPC
    sed -i.bak 's|cors_allowed_origins = \[\]|cors_allowed_origins = ["*"]|g' "${CONFIG_FILE}"
    
    # Adjust consensus timeouts for faster block times
    sed -i.bak 's|timeout_propose = "3s"|timeout_propose = "1s"|g' "${CONFIG_FILE}"
    sed -i.bak 's|timeout_prevote = "1s"|timeout_prevote = "500ms"|g' "${CONFIG_FILE}"
    sed -i.bak 's|timeout_precommit = "1s"|timeout_precommit = "500ms"|g' "${CONFIG_FILE}"
    sed -i.bak 's|timeout_commit = "5s"|timeout_commit = "1s"|g' "${CONFIG_FILE}"
    
    # Create empty blocks for development
    sed -i.bak 's|create_empty_blocks = true|create_empty_blocks = true|g' "${CONFIG_FILE}"
    sed -i.bak 's|create_empty_blocks_interval = "0s"|create_empty_blocks_interval = "30s"|g' "${CONFIG_FILE}"
    
    # Remove backup files
    rm -f "${CONFIG_FILE}.bak"
    
    # Create Gridway directories
    GRIDWAY_DIR="${TESTNET_DIR}/gridway-${i}"
    mkdir -p "${GRIDWAY_DIR}/config"
    mkdir -p "${GRIDWAY_DIR}/data"
    
    # Create Gridway configuration
    cat > "${GRIDWAY_DIR}/config/config.toml" << EOF
# Gridway Application Configuration for Node ${i}
listen_address = "tcp://0.0.0.0:26658"
grpc_address = "0.0.0.0:9090"
max_connections = 10
flush_interval = 100
persist_interval = 1
retain_blocks = 0
chain_id = "${CHAIN_ID}"
EOF

    # Create app genesis state
    cat > "${GRIDWAY_DIR}/genesis.json" << EOF
{
  "chain_id": "${CHAIN_ID}",
  "app_state": {
    "auth": {
      "accounts": []
    },
    "bank": {
      "balances": []
    }
  }
}
EOF
done

# Update persistent peers in all nodes
echo "Configuring peer connections..."
PEERS=""
for i in $(seq 0 $((NUM_NODES-1))); do
    NODE_ID=$(docker run --rm -v "$(pwd)/${TESTNET_DIR}/node${i}":/cometbft \
        cometbft/cometbft:v0.38.0 \
        show-node-id --home /cometbft)
    
    if [ -n "${PEERS}" ]; then
        PEERS="${PEERS},"
    fi
    PEERS="${PEERS}${NODE_ID}@cometbft-${i}:26656"
done

# Update persistent_peers in all nodes
for i in $(seq 0 $((NUM_NODES-1))); do
    CONFIG_FILE="${TESTNET_DIR}/node${i}/config/config.toml"
    sed -i.bak "s|persistent_peers = \"\"|persistent_peers = \"${PEERS}\"|g" "${CONFIG_FILE}"
    rm -f "${CONFIG_FILE}.bak"
done

# Set proper permissions
chmod -R 755 "${TESTNET_DIR}"

# Display summary
echo ""
echo "Multi-node testnet initialization complete!"
echo ""
echo "Network topology:"
echo "  4 validator nodes with equal voting power"
echo "  Chain ID: ${CHAIN_ID}"
echo ""
echo "Node endpoints:"
for i in $(seq 0 $((NUM_NODES-1))); do
    echo "  Node ${i}:"
    echo "    CometBFT RPC: http://localhost:$((26657 + i*10))"
    echo "    CometBFT P2P: http://localhost:$((26656 + i*10))"
    echo "    Gridway ABCI:  tcp://localhost:$((26658 + i*10))"
    echo "    Gridway gRPC:  http://localhost:$((9090 + i))"
    echo "    Gridway REST:  http://localhost:$((1317 + i))"
done
echo ""
echo "To start the multi-node testnet, run:"
echo "  docker compose -f docker-compose.multi.yml up"