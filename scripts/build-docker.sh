#!/usr/bin/env bash
set -euo pipefail

# build-docker.sh — Build the gridway Docker image.
#
# Usage:
#   ./scripts/build-docker.sh [TAG]
#
# Default tag: gridway:latest

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

TAG="${1:-gridway:latest}"

echo "=== Building Gridway Docker Image ==="
echo "  Context: $PROJECT_DIR"
echo "  Tag:     $TAG"
echo ""

docker build \
    -t "$TAG" \
    -f "$PROJECT_DIR/Dockerfile" \
    "$PROJECT_DIR"

echo ""
echo "=== Build Complete ==="
echo "  Image: $TAG"
echo ""
echo "To run testnet:"
echo "  1. ./scripts/setup-testnet.sh"
echo "  2. docker compose -f docker-compose.multi.yml up -d"
