#!/bin/bash
# Reset testnet by cleaning all blockchain data

set -e

# Load environment variables
if [ -f .env ]; then
    source .env
else
    echo "Warning: .env file not found, using defaults"
fi

# Set defaults if not provided
TESTNET_DIR=${TESTNET_DIR:-"./testnet"}
COMETBFT_HOME=${COMETBFT_HOME:-"${TESTNET_DIR}/node0"}
GRIDWAY_HOME=${GRIDWAY_HOME:-"${TESTNET_DIR}/gridway"}

echo "Resetting testnet..."

# Stop any running containers
echo "Stopping any running containers..."
docker compose down 2>/dev/null || true

# Check if testnet directory exists
if [ ! -d "${TESTNET_DIR}" ]; then
    echo "Testnet directory not found: ${TESTNET_DIR}"
    echo "Nothing to reset."
    exit 0
fi

# Backup configuration files
echo "Backing up configuration files..."
BACKUP_DIR="${TESTNET_DIR}_backup_$(date +%Y%m%d_%H%M%S)"
mkdir -p "${BACKUP_DIR}"

# Backup CometBFT config
if [ -d "${COMETBFT_HOME}/config" ]; then
    cp -r "${COMETBFT_HOME}/config" "${BACKUP_DIR}/cometbft_config"
    echo "CometBFT configuration backed up to ${BACKUP_DIR}/cometbft_config"
fi

# Backup Helium config
if [ -d "${GRIDWAY_HOME}/config" ]; then
    cp -r "${GRIDWAY_HOME}/config" "${BACKUP_DIR}/gridway_config"
    echo "Gridway configuration backed up to ${BACKUP_DIR}/gridway_config"
fi

# Clean data directories
echo "Cleaning blockchain data..."

# Clean CometBFT data
if [ -d "${COMETBFT_HOME}/data" ]; then
    rm -rf "${COMETBFT_HOME}/data"
    mkdir -p "${COMETBFT_HOME}/data"
    echo "CometBFT data cleaned"
fi

# Clean Helium data
if [ -d "${GRIDWAY_HOME}/data" ]; then
    rm -rf "${GRIDWAY_HOME}/data"
    mkdir -p "${GRIDWAY_HOME}/data"
    echo "Gridway data cleaned"
fi

# Reset CometBFT state
if [ -f "${COMETBFT_HOME}/config/priv_validator_state.json" ]; then
    echo '{"height":"0","round":0,"step":0}' > "${COMETBFT_HOME}/config/priv_validator_state.json"
    echo "Validator state reset"
fi

# Remove any Docker volumes
echo "Removing Docker volumes..."
docker volume rm gridway-worktrees_cometbft-data gridway-worktrees_gridway-data 2>/dev/null || true

echo ""
echo "Testnet reset complete!"
echo ""
echo "Configuration files have been backed up to: ${BACKUP_DIR}"
echo ""
echo "To reinitialize the testnet, run:"
echo "  ./scripts/init-testnet.sh"
echo ""
echo "To restore from backup configuration:"
echo "  cp -r ${BACKUP_DIR}/cometbft_config/* ${COMETBFT_HOME}/config/"
echo "  cp -r ${BACKUP_DIR}/gridway_config/* ${GRIDWAY_HOME}/config/"