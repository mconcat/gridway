#!/usr/bin/env bash
set -euo pipefail

# setup-testnet.sh — Generate config files for a 4-node gridway testnet.
#
# Usage:
#   ./scripts/setup-testnet.sh [NUM_VALIDATORS] [CHAIN_ID]
#
# Defaults: 4 validators, chain-id "gridway-1"
# Requires: gridway-setup binary (cargo build first, or use from Docker image)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

NUM_VALIDATORS="${1:-4}"
CHAIN_ID="${2:-gridway-1}"
OUTPUT_DIR="$PROJECT_DIR/testnet"
BOOTSTRAPPERS=1

echo "=== Gridway Testnet Setup ==="
echo "  Validators:    $NUM_VALIDATORS"
echo "  Bootstrappers: $BOOTSTRAPPERS"
echo "  Chain ID:      $CHAIN_ID"
echo "  Output:        $OUTPUT_DIR"
echo ""

# Clean previous testnet data
if [ -d "$OUTPUT_DIR" ]; then
    echo "Removing existing testnet directory..."
    rm -rf "$OUTPUT_DIR"
fi

# Find gridway-setup binary
SETUP_BIN=""
if command -v gridway-setup &>/dev/null; then
    SETUP_BIN="gridway-setup"
elif [ -x "$PROJECT_DIR/target/release/gridway-setup" ]; then
    SETUP_BIN="$PROJECT_DIR/target/release/gridway-setup"
elif [ -x "$PROJECT_DIR/target/debug/gridway-setup" ]; then
    SETUP_BIN="$PROJECT_DIR/target/debug/gridway-setup"
else
    echo "gridway-setup not found. Building..."
    cargo build --release -p gridway-consensus --bin gridway-setup
    SETUP_BIN="$PROJECT_DIR/target/release/gridway-setup"
fi

echo "Using: $SETUP_BIN"
echo ""

# Run gridway-setup
"$SETUP_BIN" \
    --peers="$NUM_VALIDATORS" \
    --bootstrappers="$BOOTSTRAPPERS" \
    --output="$OUTPUT_DIR" \
    --chain-id="$CHAIN_ID"

echo ""
echo "=== Testnet Setup Complete ==="
echo ""
echo "Generated files:"
find "$OUTPUT_DIR" -type f | sort | sed 's/^/  /'
echo ""
echo "To start single-node (validator-0):"
echo "  docker compose up -d"
echo ""
echo "To start 4-node testnet:"
echo "  docker compose -f docker-compose.multi.yml up -d"
echo ""
echo "To monitor logs:"
echo "  docker compose -f docker-compose.multi.yml logs -f"
echo ""
echo "HTTP API endpoints (4-node):"
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    port=$((4547 + i * 3))
    echo "  Node $i: http://localhost:$port"
done
