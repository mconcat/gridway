#!/bin/bash
# E2E test: state sync — block replay on restart + state snapshots
#
# This script:
#   1. Builds the binaries
#   2. Sets up a 5-node testnet
#   3. Starts the nodes, waits for consensus
#   4. Submits transactions, verifies balances changed
#   5. Stops all nodes
#   6. Restarts all nodes
#   7. Verifies balances are preserved (state was replayed from blocks)
#   8. Tests /snapshot and /status endpoints

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

SETUP_DIR="$PROJECT_DIR/.test-state-sync"
KEYGEN="$PROJECT_DIR/target/debug/gridway-keygen"
SETUP_BIN="$PROJECT_DIR/target/debug/gridway-setup"
NODE_BIN="$PROJECT_DIR/target/debug/gridway-node"
CARGO_HOME="${CARGO_HOME:-$PROJECT_DIR/.cargo-local}"
export CARGO_HOME

PASS=0
FAIL=0
PIDS=()

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

check() {
    local desc="$1"
    local expected="$2"
    local actual="$3"
    if [ "$expected" = "$actual" ]; then
        echo -e "  ${GREEN}✓${NC} $desc (got: $actual)"
        ((PASS++))
    else
        echo -e "  ${RED}✗${NC} $desc (expected: $expected, got: $actual)"
        ((FAIL++))
    fi
}

cleanup() {
    echo ""
    echo "Cleaning up..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    # Don't rm setup dir — keep for debugging on failure
}
trap cleanup EXIT

# ============================================================================
# Step 0: Build
# ============================================================================
echo "============================================"
echo " Gridway State Sync E2E Test"
echo "============================================"
echo ""

echo "Step 0: Building binaries..."
cargo build -p gridway-consensus --bin gridway-node --bin gridway-setup --bin gridway-keygen 2>&1 | tail -3
echo ""

# ============================================================================
# Step 1: Setup 5-node testnet
# ============================================================================
echo "Step 1: Setting up 5-node testnet..."
rm -rf "$SETUP_DIR"
"$SETUP_BIN" --peers 5 --bootstrappers 2 --start-port 14545 --output "$SETUP_DIR" 2>&1 | grep -v "^$"
echo ""

PEERS_FILE="$SETUP_DIR/peers.yaml"
TX_PORT=14547  # first node's tx port (start_port + 2)
BASE_URL="http://localhost:${TX_PORT}"

# ============================================================================
# Step 2: Start all 5 nodes
# ============================================================================
echo "Step 2: Starting 5 nodes..."
for config_file in "$SETUP_DIR"/*.yaml; do
    [ "$(basename "$config_file")" = "peers.yaml" ] && continue
    "$NODE_BIN" --peers "$PEERS_FILE" --config "$config_file" > /dev/null 2>&1 &
    PIDS+=($!)
done
echo "  Started ${#PIDS[@]} nodes"

# Wait for HTTP API
echo "  Waiting for HTTP API..."
for i in $(seq 1 30); do
    if curl -s "${BASE_URL}/health" >/dev/null 2>&1; then
        echo -e "  ${GREEN}✓${NC} Node API ready (attempt $i)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo -e "  ${RED}✗${NC} Node not ready after 30 attempts"
        exit 1
    fi
    sleep 1
done
echo ""

# ============================================================================
# Step 3: Generate keypairs and check genesis balances
# ============================================================================
echo "Step 3: Check genesis balances"

ALICE_JSON=$("$KEYGEN" generate --seed 1)
ALICE_PRIVKEY=$(echo "$ALICE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['private_key'])")
ALICE_PUBKEY=$(echo "$ALICE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['public_key'])")
ALICE_ADDRESS=$(echo "$ALICE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])")

BOB_JSON=$("$KEYGEN" generate --seed 2)
BOB_PRIVKEY=$(echo "$BOB_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['private_key'])")
BOB_PUBKEY=$(echo "$BOB_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['public_key'])")
BOB_ADDRESS=$(echo "$BOB_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])")

echo "  Alice: $ALICE_ADDRESS"
echo "  Bob:   $BOB_ADDRESS"

ALICE_BAL=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 1000000 ugridway" "1000000" "$ALICE_BAL"

BOB_BAL=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 0 ugridway" "0" "$BOB_BAL"
echo ""

# ============================================================================
# Step 4: Submit transactions
# ============================================================================
echo "Step 4: Submit transfer (alice → bob, 100 ugridway)"

TX_BODY='{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${ALICE_ADDRESS}"'","to_address":"'"${BOB_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"100"}]}],"chain_id":"gridway-1","sequence":0,"memo":""}'
SIGNED_TX=$("$KEYGEN" sign --key "$ALICE_PRIVKEY" --body "$TX_BODY")
SUBMIT_RESULT=$(curl -s -X POST "${BASE_URL}/tx" -H "Content-Type: application/json" -d "$SIGNED_TX")
SUBMIT_STATUS=$(echo "$SUBMIT_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null || echo "ERROR")
check "tx submitted" "submitted" "$SUBMIT_STATUS"

echo "  Waiting for finalization (8 seconds)..."
sleep 8

ALICE_BAL2=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 999900 ugridway" "999900" "$ALICE_BAL2"

BOB_BAL2=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 100 ugridway" "100" "$BOB_BAL2"
echo ""

# ============================================================================
# Step 5: Test /status endpoint (before restart)
# ============================================================================
echo "Step 5: Test /status endpoint"
STATUS_RESP=$(curl -s "${BASE_URL}/status")
STATE_ROOT_BEFORE=$(echo "$STATUS_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('state_root',''))" 2>/dev/null || echo "ERROR")
if [ "$STATE_ROOT_BEFORE" != "ERROR" ] && [ -n "$STATE_ROOT_BEFORE" ] && [ "$STATE_ROOT_BEFORE" != "0000000000000000000000000000000000000000000000000000000000000000" ]; then
    echo -e "  ${GREEN}✓${NC} /status returns state_root: ${STATE_ROOT_BEFORE:0:16}..."
    ((PASS++))
else
    echo -e "  ${RED}✗${NC} /status returned invalid state_root: $STATE_ROOT_BEFORE"
    ((FAIL++))
fi
echo ""

# ============================================================================
# Step 6: Test /snapshot endpoint (before restart)
# ============================================================================
echo "Step 6: Test /snapshot endpoint"
SNAPSHOT_RESP=$(curl -s "${BASE_URL}/snapshot")
SNAPSHOT_OK=$(echo "$SNAPSHOT_RESP" | python3 -c "
import sys,json
d=json.load(sys.stdin)
if 'entries' in d and 'root_hash' in d and 'version' in d:
    print(f'OK entries={len(d[\"entries\"])} version={d[\"version\"]}')
else:
    print('FAIL')
" 2>/dev/null || echo "FAIL")
if [[ "$SNAPSHOT_OK" == OK* ]]; then
    echo -e "  ${GREEN}✓${NC} /snapshot returns valid snapshot: $SNAPSHOT_OK"
    ((PASS++))
else
    echo -e "  ${RED}✗${NC} /snapshot returned invalid data"
    ((FAIL++))
fi
echo ""

# ============================================================================
# Step 7: Stop all nodes
# ============================================================================
echo "Step 7: Stopping all nodes..."
for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
done
wait 2>/dev/null || true
PIDS=()
sleep 2
echo -e "  ${GREEN}✓${NC} All nodes stopped"
echo ""

# ============================================================================
# Step 8: Restart all nodes (state should be replayed from blocks)
# ============================================================================
echo "Step 8: Restarting all nodes (state replay from archive)..."
for config_file in "$SETUP_DIR"/*.yaml; do
    [ "$(basename "$config_file")" = "peers.yaml" ] && continue
    "$NODE_BIN" --peers "$PEERS_FILE" --config "$config_file" > /dev/null 2>&1 &
    PIDS+=($!)
done
echo "  Started ${#PIDS[@]} nodes"

# Wait for HTTP API
echo "  Waiting for HTTP API..."
for i in $(seq 1 30); do
    if curl -s "${BASE_URL}/health" >/dev/null 2>&1; then
        echo -e "  ${GREEN}✓${NC} Node API ready (attempt $i)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo -e "  ${RED}✗${NC} Node not ready after 30 attempts"
        exit 1
    fi
    sleep 1
done
echo ""

# ============================================================================
# Step 9: Verify state was preserved through restart
# ============================================================================
echo "Step 9: Verify state survived restart (block replay)"

# Give a moment for state to settle
sleep 2

ALICE_BAL3=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice still has 999900 ugridway after restart" "999900" "$ALICE_BAL3"

BOB_BAL3=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob still has 100 ugridway after restart" "100" "$BOB_BAL3"

ALICE_SEQ=$(curl -s "${BASE_URL}/account/${ALICE_ADDRESS}" | python3 -c "import sys,json; print(json.load(sys.stdin)['sequence'])" 2>/dev/null || echo "ERROR")
check "alice sequence is 1 after restart" "1" "$ALICE_SEQ"
echo ""

# ============================================================================
# Step 10: Verify state root matches after restart
# ============================================================================
echo "Step 10: Verify state root matches"
STATUS_AFTER=$(curl -s "${BASE_URL}/status")
STATE_ROOT_AFTER=$(echo "$STATUS_AFTER" | python3 -c "import sys,json; print(json.load(sys.stdin).get('state_root',''))" 2>/dev/null || echo "ERROR")
check "state root matches after restart" "$STATE_ROOT_BEFORE" "$STATE_ROOT_AFTER"
echo ""

# ============================================================================
# Step 11: Submit another transaction after restart (proves live consensus)
# ============================================================================
echo "Step 11: Submit another transfer after restart (proves live consensus)"
TX_BODY2='{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${ALICE_ADDRESS}"'","to_address":"'"${BOB_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"50"}]}],"chain_id":"gridway-1","sequence":1,"memo":""}'
SIGNED_TX2=$("$KEYGEN" sign --key "$ALICE_PRIVKEY" --body "$TX_BODY2")
SUBMIT_RESULT2=$(curl -s -X POST "${BASE_URL}/tx" -H "Content-Type: application/json" -d "$SIGNED_TX2")
SUBMIT_STATUS2=$(echo "$SUBMIT_RESULT2" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null || echo "ERROR")
check "post-restart tx submitted" "submitted" "$SUBMIT_STATUS2"

echo "  Waiting for finalization (8 seconds)..."
sleep 8

ALICE_BAL4=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 999850 ugridway" "999850" "$ALICE_BAL4"

BOB_BAL4=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 150 ugridway" "150" "$BOB_BAL4"
echo ""

# ============================================================================
# Summary
# ============================================================================
echo "============================================"
echo " Results: ${PASS} passed, ${FAIL} failed"
echo "============================================"

# Cleanup test dir on success
if [ "$FAIL" -eq 0 ]; then
    rm -rf "$SETUP_DIR"
    echo -e "${GREEN}ALL TESTS PASSED — state sync works!${NC}"
else
    echo -e "${RED}SOME TESTS FAILED${NC}"
    echo "  Test data preserved at: $SETUP_DIR"
    exit 1
fi
