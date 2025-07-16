#!/bin/bash
# Initialize a single-node CometBFT testnet with Helium application

set -e

# Load environment variables
if [ -f .env ]; then
    source .env
else
    echo "Warning: .env file not found, using defaults"
fi

# Set defaults if not provided
CHAIN_ID=${CHAIN_ID:-"helium-testnet"}
TESTNET_DIR=${TESTNET_DIR:-"./testnet"}
COMETBFT_HOME=${COMETBFT_HOME:-"${TESTNET_DIR}/node0"}
HELIUM_HOME=${HELIUM_HOME:-"${TESTNET_DIR}/helium"}
MONIKER=${MONIKER:-"helium-node-0"}
VALIDATOR_KEY_ALGO=${VALIDATOR_KEY_ALGO:-"secp256k1"}

echo "Initializing testnet..."
echo "Chain ID: ${CHAIN_ID}"
echo "Testnet directory: ${TESTNET_DIR}"

# Clean existing data
if [ -d "${TESTNET_DIR}" ]; then
    echo "Removing existing testnet directory..."
    rm -rf "${TESTNET_DIR}"
fi

# Create directories
echo "Creating testnet directories..."
mkdir -p "${COMETBFT_HOME}/config"
mkdir -p "${COMETBFT_HOME}/data"
mkdir -p "${HELIUM_HOME}/config"
mkdir -p "${HELIUM_HOME}/data"

# Set permissions for Docker access
chmod -R 777 "${TESTNET_DIR}"

# Initialize CometBFT configuration
echo "Initializing CometBFT configuration..."
docker run --rm -v "$(pwd)/${COMETBFT_HOME}":/cometbft \
    cometbft/cometbft:v0.38.0 \
    init --home /cometbft

# Fix permissions after Docker creates files
chmod -R 777 "${TESTNET_DIR}"

# Update CometBFT config
echo "Updating CometBFT configuration..."
cat > "${COMETBFT_HOME}/config/config.toml" << EOF
# This is a TOML config file for CometBFT.
# For more information, see https://docs.cometbft.com/

proxy_app = "tcp://helium:26658"
moniker = "${MONIKER}"

[rpc]
laddr = "tcp://0.0.0.0:26657"
cors_allowed_origins = ["*"]
cors_allowed_methods = ["HEAD", "GET", "POST"]
cors_allowed_headers = ["Origin", "Accept", "Content-Type", "X-Requested-With", "X-Server-Time"]

[p2p]
laddr = "tcp://0.0.0.0:26656"
persistent_peers = ""

[mempool]
size = 5000
cache_size = 10000

[consensus]
create_empty_blocks = true
create_empty_blocks_interval = "30s"
timeout_propose = "3s"
timeout_propose_delta = "500ms"
timeout_prevote = "1s"
timeout_prevote_delta = "500ms"
timeout_precommit = "1s"
timeout_precommit_delta = "500ms"
timeout_commit = "5s"

[tx_index]
indexer = "kv"
EOF

# Create genesis file
echo "Creating genesis file..."
cat > "${COMETBFT_HOME}/config/genesis.json" << EOF
{
  "genesis_time": "$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || date '+%Y-%m-%dT%H:%M:%SZ')",
  "chain_id": "${CHAIN_ID}",
  "initial_height": "1",
  "consensus_params": {
    "block": {
      "max_bytes": "22020096",
      "max_gas": "-1"
    },
    "evidence": {
      "max_age_num_blocks": "100000",
      "max_age_duration": "172800000000000",
      "max_bytes": "1048576"
    },
    "validator": {
      "pub_key_types": [
        "ed25519"
      ]
    },
    "version": {
      "app": "0"
    }
  },
  "validators": [],
  "app_hash": "",
  "app_state": {}
}
EOF

# Generate validator key
echo "Generating validator key..."
docker run --rm -v "$(pwd)/${COMETBFT_HOME}":/cometbft \
    cometbft/cometbft:v0.38.0 \
    gen-validator --home /cometbft

# Fix permissions after Docker command
chmod -R 777 "${TESTNET_DIR}"

# Extract validator public key and create validator entry
VALIDATOR_PUBKEY=$(docker run --rm -v "$(pwd)/${COMETBFT_HOME}":/cometbft \
    cometbft/cometbft:v0.38.0 \
    show-validator --home /cometbft)

# Update genesis with validator
echo "Adding validator to genesis..."
TEMP_GENESIS=$(mktemp)
jq --arg pubkey "$VALIDATOR_PUBKEY" \
   '.validators = [{
      "address": "",
      "pub_key": ($pubkey | fromjson),
      "power": "10",
      "name": "'"${MONIKER}"'"
    }]' "${COMETBFT_HOME}/config/genesis.json" > "$TEMP_GENESIS"
mv "$TEMP_GENESIS" "${COMETBFT_HOME}/config/genesis.json"

# Initialize Helium application
echo "Initializing Helium application..."
cat > "${HELIUM_HOME}/config/config.toml" << EOF
# Helium Application Configuration
listen_address = "tcp://0.0.0.0:26658"
grpc_address = "0.0.0.0:9090"
max_connections = 10
flush_interval = 100
persist_interval = 1
retain_blocks = 0
chain_id = "${CHAIN_ID}"
EOF

# Create app genesis state
echo "Creating application genesis state..."
cat > "${HELIUM_HOME}/genesis.json" << EOF
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

# Set proper permissions - ensure everything is readable/writable
chmod -R 777 "${TESTNET_DIR}"

echo ""
echo "Testnet initialization complete!"
echo ""
echo "To start the testnet, run:"
echo "  ./scripts/start-testnet.sh"
echo ""
echo "Node information:"
echo "  Chain ID: ${CHAIN_ID}"
echo "  Moniker: ${MONIKER}"
echo "  CometBFT Home: ${COMETBFT_HOME}"
echo "  Helium Home: ${HELIUM_HOME}"
echo "  Validator Pubkey: ${VALIDATOR_PUBKEY}"