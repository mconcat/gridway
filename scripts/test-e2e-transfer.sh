#!/bin/bash
# E2E test: token transfer on 5-node testnet
#
# Prerequisites:
#   cargo build --bin gridway-setup --bin gridway-node
#   Run gridway-setup to generate configs, then start 5 nodes.
#
# This script:
#   1. Waits for the HTTP API to be ready
#   2. Checks initial balances (alice: 1_000_000, bob: 0)
#   3. Submits a MsgSend (alice → bob, 100 ugridway)
#   4. Waits for finalization
#   5. Verifies balances changed

set -euo pipefail

# Default TX port for the first node (setup uses start_port + 2)
TX_PORT="${TX_PORT:-4547}"
BASE_URL="http://localhost:${TX_PORT}"
PASS=0
FAIL=0

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

echo "============================================"
echo " Gridway E2E Token Transfer Test"
echo " API: $BASE_URL"
echo "============================================"
echo ""

# --- Step 0: Wait for node to be ready ---
echo "Waiting for node HTTP API..."
for i in $(seq 1 30); do
    if curl -s "${BASE_URL}/health" >/dev/null 2>&1; then
        echo -e "  ${GREEN}✓${NC} Node is ready (attempt $i)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo -e "  ${RED}✗${NC} Node not ready after 30 attempts"
        exit 1
    fi
    sleep 1
done
echo ""

# --- Step 1: Check initial balances ---
echo "Step 1: Check initial balances"

ALICE_BAL=$(curl -s "${BASE_URL}/balance/alice/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 1000000 ugridway" "1000000" "$ALICE_BAL"

BOB_BAL=$(curl -s "${BASE_URL}/balance/bob/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 0 ugridway" "0" "$BOB_BAL"
echo ""

# --- Step 2: Submit MsgSend (alice → bob, 100 ugridway) ---
echo "Step 2: Submit MsgSend (alice → bob, 100 ugridway)"

TX_BODY='{
  "body": {
    "messages": [{
      "@type": "/gridway.bank.v1.MsgSend",
      "from_address": "alice",
      "to_address": "bob",
      "amount": [{"denom": "ugridway", "amount": "100"}]
    }]
  }
}'

SUBMIT_RESULT=$(curl -s -X POST "${BASE_URL}/tx" \
    -H "Content-Type: application/json" \
    -d "$TX_BODY")
SUBMIT_STATUS=$(echo "$SUBMIT_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null || echo "ERROR")
check "tx submitted" "submitted" "$SUBMIT_STATUS"
echo "  Response: $SUBMIT_RESULT"
echo ""

# --- Step 3: Wait for finalization ---
echo "Step 3: Waiting for block finalization (5 seconds)..."
sleep 5
echo ""

# --- Step 4: Check balances changed ---
echo "Step 4: Check balances after transfer"

ALICE_BAL2=$(curl -s "${BASE_URL}/balance/alice/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 999900 ugridway" "999900" "$ALICE_BAL2"

BOB_BAL2=$(curl -s "${BASE_URL}/balance/bob/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 100 ugridway" "100" "$BOB_BAL2"
echo ""

# --- Step 5: Submit another transfer (bob → alice, 50 ugridway) ---
echo "Step 5: Submit reverse transfer (bob → alice, 50 ugridway)"

TX_BODY2='{
  "body": {
    "messages": [{
      "@type": "/gridway.bank.v1.MsgSend",
      "from_address": "bob",
      "to_address": "alice",
      "amount": [{"denom": "ugridway", "amount": "50"}]
    }]
  }
}'

SUBMIT_RESULT2=$(curl -s -X POST "${BASE_URL}/tx" \
    -H "Content-Type: application/json" \
    -d "$TX_BODY2")
SUBMIT_STATUS2=$(echo "$SUBMIT_RESULT2" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null || echo "ERROR")
check "reverse tx submitted" "submitted" "$SUBMIT_STATUS2"
echo ""

echo "Waiting for finalization (5 seconds)..."
sleep 5

echo "Step 6: Check final balances"

ALICE_FINAL=$(curl -s "${BASE_URL}/balance/alice/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 999950 ugridway" "999950" "$ALICE_FINAL"

BOB_FINAL=$(curl -s "${BASE_URL}/balance/bob/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 50 ugridway" "50" "$BOB_FINAL"
echo ""

# --- Step 7: Test insufficient funds ---
echo "Step 7: Test insufficient funds (bob sends 1000000)"

TX_BODY3='{
  "body": {
    "messages": [{
      "@type": "/gridway.bank.v1.MsgSend",
      "from_address": "bob",
      "to_address": "alice",
      "amount": [{"denom": "ugridway", "amount": "1000000"}]
    }]
  }
}'

curl -s -X POST "${BASE_URL}/tx" -H "Content-Type: application/json" -d "$TX_BODY3" >/dev/null
echo "  (submitted — should fail during execution, balances should not change)"
sleep 5

BOB_AFTER_FAIL=$(curl -s "${BASE_URL}/balance/bob/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob still has 50 ugridway (insufficient funds rejected)" "50" "$BOB_AFTER_FAIL"
echo ""

# --- Summary ---
echo "============================================"
echo " Results: ${PASS} passed, ${FAIL} failed"
echo "============================================"

if [ "$FAIL" -gt 0 ]; then
    echo -e "${RED}SOME TESTS FAILED${NC}"
    exit 1
else
    echo -e "${GREEN}ALL TESTS PASSED${NC}"
fi
