#!/bin/bash
# Start the CometBFT testnet with Helium application

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
HELIUM_HOME=${HELIUM_HOME:-"${TESTNET_DIR}/helium"}

# Check if testnet is initialized
if [ ! -d "${TESTNET_DIR}" ]; then
    echo "Error: Testnet not initialized. Please run ./scripts/init-testnet.sh first"
    exit 1
fi

if [ ! -f "${COMETBFT_HOME}/config/genesis.json" ]; then
    echo "Error: Genesis file not found. Please run ./scripts/init-testnet.sh first"
    exit 1
fi

# Function to check if services are healthy
check_services() {
    echo "Checking service health..."
    
    # Check Helium ABCI
    HELIUM_READY=false
    for i in {1..30}; do
        if nc -z localhost 26658 2>/dev/null; then
            HELIUM_READY=true
            echo "✓ Helium ABCI is ready"
            break
        fi
        echo "  Waiting for Helium ABCI to be ready... ($i/30)"
        sleep 2
    done
    
    if [ "$HELIUM_READY" = false ]; then
        echo "✗ Helium ABCI failed to start"
        return 1
    fi
    
    # Check CometBFT RPC
    COMETBFT_READY=false
    for i in {1..30}; do
        if nc -z localhost 26657 2>/dev/null; then
            COMETBFT_READY=true
            echo "✓ CometBFT RPC is ready"
            break
        fi
        echo "  Waiting for CometBFT RPC to be ready... ($i/30)"
        sleep 2
    done
    
    if [ "$COMETBFT_READY" = false ]; then
        echo "✗ CometBFT RPC failed to start"
        return 1
    fi
    
    return 0
}

# Function to display logs
show_logs() {
    echo ""
    echo "Showing logs (press Ctrl+C to stop)..."
    docker compose logs -f
}

# Function to handle shutdown
cleanup() {
    echo ""
    echo "Shutting down testnet..."
    docker compose down
    exit 0
}

# Set up signal handler
trap cleanup INT TERM

echo "Starting CometBFT testnet with Helium application..."
echo ""
echo "Configuration:"
echo "  CometBFT Home: ${COMETBFT_HOME}"
echo "  Helium Home: ${HELIUM_HOME}"
echo ""

# Copy .env file if it exists
if [ -f .env ]; then
    cp .env .env.docker
fi

# Start services
echo "Starting Docker Compose services..."
docker compose up -d

# Wait for services to be ready
if check_services; then
    echo ""
    echo "✓ Testnet started successfully!"
    echo ""
    echo "Service endpoints:"
    echo "  CometBFT RPC:  http://localhost:26657"
    echo "  CometBFT P2P:  http://localhost:26656"
    echo "  Helium ABCI:   tcp://localhost:26658"
    echo "  Helium gRPC:   http://localhost:9090"
    echo "  Helium REST:   http://localhost:1317"
    echo ""
    echo "Useful commands:"
    echo "  Check status:     curl http://localhost:26657/status"
    echo "  View logs:        docker compose logs -f"
    echo "  Stop testnet:     docker compose down"
    echo "  Reset testnet:    ./scripts/reset-testnet.sh"
    echo ""
    
    # Ask if user wants to see logs
    read -p "Would you like to follow the logs? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        show_logs
    else
        echo "Testnet is running in the background."
        echo "Use 'docker compose logs -f' to view logs."
    fi
else
    echo ""
    echo "✗ Failed to start testnet properly"
    echo ""
    echo "Checking container status..."
    docker compose ps
    echo ""
    echo "Recent logs:"
    docker compose logs --tail=50
    exit 1
fi