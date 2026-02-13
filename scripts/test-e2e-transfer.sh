#!/bin/bash
# E2E test: signed token transfer on 5-node testnet
#
# Prerequisites:
#   cargo build --bin gridway-setup --bin gridway-node --bin gridway-keygen
#   Run gridway-setup to generate configs, then start 5 nodes.
#
# This script:
#   1. Generates deterministic keypairs (must match genesis seeds)
#   2. Waits for the HTTP API to be ready
#   3. Checks initial balances
#   4. Signs and submits transactions using gridway-keygen
#   5. Verifies balances changed
#   6. Tests invalid signatures and wrong sequences are rejected

set -euo pipefail

# Default TX port for the first node (setup uses start_port + 2)
TX_PORT="${TX_PORT:-4547}"
BASE_URL="http://localhost:${TX_PORT}"
PASS=0
FAIL=0

# Path to keygen binary
KEYGEN="${KEYGEN:-cargo run --bin gridway-keygen --}"

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
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}✗${NC} $desc (expected: $expected, got: $actual)"
        FAIL=$((FAIL + 1))
    fi
}


# Poll until a balance reaches the expected value (or timeout)
wait_for_balance() {
    local address="$1"
    local denom="$2"
    local expected="$3"
    local max_wait="${4:-30}"
    for i in $(seq 1 "$max_wait"); do
        local bal
        bal=$(curl -s "${BASE_URL}/balance/${address}/${denom}" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
        if [ "$bal" = "$expected" ]; then
            return 0
        fi
        sleep 1
    done
    return 1
}

echo "============================================"
echo " Gridway E2E Signed Token Transfer Test"
echo " API: $BASE_URL"
echo "============================================"
echo ""

# --- Step 0: Generate keypairs with deterministic seeds (must match genesis) ---
echo "Step 0: Generate keypairs (deterministic seeds)"

ALICE_JSON=$($KEYGEN generate --seed 1)
ALICE_PRIVKEY=$(echo "$ALICE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['private_key'])")
ALICE_PUBKEY=$(echo "$ALICE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['public_key'])")
ALICE_ADDRESS=$(echo "$ALICE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])")
echo "  Alice address: $ALICE_ADDRESS"

BOB_JSON=$($KEYGEN generate --seed 2)
BOB_PRIVKEY=$(echo "$BOB_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['private_key'])")
BOB_PUBKEY=$(echo "$BOB_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['public_key'])")
BOB_ADDRESS=$(echo "$BOB_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])")
echo "  Bob address:   $BOB_ADDRESS"
echo ""

# --- Step 1: Wait for node to be ready ---
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

# --- Step 2: Check initial balances ---
echo "Step 1: Check initial balances"

ALICE_BAL=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 1000000 ugridway" "1000000" "$ALICE_BAL"

BOB_BAL=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 0 ugridway" "0" "$BOB_BAL"
echo ""

# --- Step 3: Check accounts ---
echo "Step 2: Check accounts exist"

ALICE_SEQ=$(curl -s "${BASE_URL}/account/${ALICE_ADDRESS}" | python3 -c "import sys,json; print(json.load(sys.stdin)['sequence'])" 2>/dev/null || echo "ERROR")
check "alice sequence is 0" "0" "$ALICE_SEQ"

BOB_SEQ=$(curl -s "${BASE_URL}/account/${BOB_ADDRESS}" | python3 -c "import sys,json; print(json.load(sys.stdin)['sequence'])" 2>/dev/null || echo "ERROR")
check "bob sequence is 0" "0" "$BOB_SEQ"
echo ""

# --- Step 4: Submit signed MsgSend (alice → bob, 100 ugridway) ---
echo "Step 3: Submit signed MsgSend (alice → bob, 100 ugridway)"

TX_BODY='{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${ALICE_ADDRESS}"'","to_address":"'"${BOB_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"100"}]}],"chain_id":"gridway-1","sequence":0,"memo":""}'

SIGNED_TX=$($KEYGEN sign --key "$ALICE_PRIVKEY" --body "$TX_BODY")
echo "  Signed TX: ${SIGNED_TX:0:100}..."

SUBMIT_RESULT=$(curl -s -X POST "${BASE_URL}/tx" \
    -H "Content-Type: application/json" \
    -d "$SIGNED_TX")
SUBMIT_STATUS=$(echo "$SUBMIT_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null || echo "ERROR")
check "tx submitted" "submitted" "$SUBMIT_STATUS"
echo ""

# --- Step 5: Wait for finalization ---
echo "Step 4: Waiting for block finalization (polling up to 30s)..."
wait_for_balance "$ALICE_ADDRESS" "ugridway" "999900" 30 || echo -e "  ${YELLOW}⚠${NC} Timed out waiting for balance change"
echo ""

# --- Step 6: Check balances changed ---
echo "Step 5: Check balances after transfer"

ALICE_BAL2=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 999900 ugridway" "999900" "$ALICE_BAL2"

BOB_BAL2=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 100 ugridway" "100" "$BOB_BAL2"
echo ""

# --- Step 7: Check alice sequence incremented ---
echo "Step 6: Check alice sequence incremented"
ALICE_SEQ2=$(curl -s "${BASE_URL}/account/${ALICE_ADDRESS}" | python3 -c "import sys,json; print(json.load(sys.stdin)['sequence'])" 2>/dev/null || echo "ERROR")
check "alice sequence is 1" "1" "$ALICE_SEQ2"
echo ""

# --- Step 8: Submit reverse transfer (bob → alice, 50 ugridway) ---
echo "Step 7: Submit reverse transfer (bob → alice, 50 ugridway)"

TX_BODY2='{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${BOB_ADDRESS}"'","to_address":"'"${ALICE_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"50"}]}],"chain_id":"gridway-1","sequence":0,"memo":""}'

SIGNED_TX2=$($KEYGEN sign --key "$BOB_PRIVKEY" --body "$TX_BODY2")

SUBMIT_RESULT2=$(curl -s -X POST "${BASE_URL}/tx" \
    -H "Content-Type: application/json" \
    -d "$SIGNED_TX2")
SUBMIT_STATUS2=$(echo "$SUBMIT_RESULT2" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null || echo "ERROR")
check "reverse tx submitted" "submitted" "$SUBMIT_STATUS2"
echo ""

echo "Waiting for finalization (polling up to 30s)..."
wait_for_balance "$ALICE_ADDRESS" "ugridway" "999950" 30 || echo -e "  ${YELLOW}⚠${NC} Timed out waiting for balance change"

echo "Step 8: Check final balances"

ALICE_FINAL=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice has 999950 ugridway" "999950" "$ALICE_FINAL"

BOB_FINAL=$(curl -s "${BASE_URL}/balance/${BOB_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "bob has 50 ugridway" "50" "$BOB_FINAL"
echo ""

# --- Step 9: Test wrong signature (should be rejected) ---
echo "Step 9: Test wrong signature (bob signs alice's tx)"

TX_BODY_WRONG='{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${ALICE_ADDRESS}"'","to_address":"'"${BOB_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"100"}]}],"chain_id":"gridway-1","sequence":1,"memo":""}'

# Sign with bob's key but alice's from_address
WRONG_SIGNED=$($KEYGEN sign --key "$BOB_PRIVKEY" --body "$TX_BODY_WRONG")
curl -s -X POST "${BASE_URL}/tx" -H "Content-Type: application/json" -d "$WRONG_SIGNED" >/dev/null
echo "  (submitted TX signed by wrong key — should fail during execution)"
sleep 5

ALICE_AFTER_WRONG=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice still has 999950 ugridway (wrong sig rejected)" "999950" "$ALICE_AFTER_WRONG"
echo ""

# --- Step 10: Test wrong sequence (should be rejected) ---
echo "Step 10: Test wrong sequence number"

TX_BODY_BAD_SEQ='{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${ALICE_ADDRESS}"'","to_address":"'"${BOB_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"100"}]}],"chain_id":"gridway-1","sequence":999,"memo":""}'

WRONG_SEQ_TX=$($KEYGEN sign --key "$ALICE_PRIVKEY" --body "$TX_BODY_BAD_SEQ")
curl -s -X POST "${BASE_URL}/tx" -H "Content-Type: application/json" -d "$WRONG_SEQ_TX" >/dev/null
echo "  (submitted TX with wrong sequence — should fail during execution)"
sleep 5

ALICE_AFTER_BAD_SEQ=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice still has 999950 ugridway (wrong seq rejected)" "999950" "$ALICE_AFTER_BAD_SEQ"
echo ""

# --- Step 11: Test unsigned TX (should be rejected) ---
echo "Step 11: Test unsigned TX (no signature field)"

UNSIGNED_TX='{"body":{"messages":[{"@type":"/gridway.bank.v1.MsgSend","from_address":"'"${ALICE_ADDRESS}"'","to_address":"'"${BOB_ADDRESS}"'","amount":[{"denom":"ugridway","amount":"100"}]}]}}'
curl -s -X POST "${BASE_URL}/tx" -H "Content-Type: application/json" -d "$UNSIGNED_TX" >/dev/null
echo "  (submitted unsigned TX — should fail)"
sleep 5

ALICE_AFTER_UNSIGNED=$(curl -s "${BASE_URL}/balance/${ALICE_ADDRESS}/ugridway" | python3 -c "import sys,json; print(json.load(sys.stdin)['balance'])" 2>/dev/null || echo "ERROR")
check "alice still has 999950 ugridway (unsigned rejected)" "999950" "$ALICE_AFTER_UNSIGNED"
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
