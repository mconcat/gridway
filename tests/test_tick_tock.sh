#!/bin/bash
# Test suite for tick-tock development methodology scripts

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Get the directory where this script is located
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$TEST_DIR")"
SCRIPTS_DIR="$ROOT_DIR/scripts"

# Create temporary directory for test files
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Helper functions
print_test_header() {
    echo -e "\n${YELLOW}TEST: $1${NC}"
    ((TESTS_RUN++))
}

test_pass() {
    echo -e "${GREEN}✓ PASS${NC}: $1"
    ((TESTS_PASSED++))
}

test_fail() {
    echo -e "${RED}✗ FAIL${NC}: $1"
    ((TESTS_FAILED++))
}

# Test generate-claude-md.py with valid inputs
test_generate_claude_md_valid() {
    print_test_header "generate-claude-md.py with valid inputs"
    
    # Test tick stage
    if python3 "$SCRIPTS_DIR/generate-claude-md.py" tick >/dev/null 2>&1; then
        if grep -q "You are currently in TICK STAGE" "$ROOT_DIR/CLAUDE.md"; then
            test_pass "tick stage generation"
        else
            test_fail "tick stage marker not found in CLAUDE.md"
        fi
    else
        test_fail "tick stage generation failed"
    fi
    
    # Test tock stage
    if python3 "$SCRIPTS_DIR/generate-claude-md.py" tock >/dev/null 2>&1; then
        if grep -q "You are currently in TOCK STAGE" "$ROOT_DIR/CLAUDE.md"; then
            test_pass "tock stage generation"
        else
            test_fail "tock stage marker not found in CLAUDE.md"
        fi
    else
        test_fail "tock stage generation failed"
    fi
}

# Test generate-claude-md.py with invalid inputs
test_generate_claude_md_invalid() {
    print_test_header "generate-claude-md.py with invalid inputs"
    
    # Test invalid stage
    if ! python3 "$SCRIPTS_DIR/generate-claude-md.py" invalid >/dev/null 2>&1; then
        test_pass "rejected invalid stage"
    else
        test_fail "accepted invalid stage"
    fi
    
    # Test no arguments
    if ! python3 "$SCRIPTS_DIR/generate-claude-md.py" >/dev/null 2>&1; then
        test_pass "rejected missing arguments"
    else
        test_fail "accepted missing arguments"
    fi
    
    # Test path traversal attempt
    if ! python3 "$SCRIPTS_DIR/generate-claude-md.py" "../tick" >/dev/null 2>&1; then
        test_pass "rejected path traversal"
    else
        test_fail "accepted path traversal"
    fi
}

# Test tick-command.sh
test_tick_command() {
    print_test_header "tick-command.sh"
    
    # Make backup of current CLAUDE.md if it exists
    if [[ -f "$ROOT_DIR/CLAUDE.md" ]]; then
        cp "$ROOT_DIR/CLAUDE.md" "$TEMP_DIR/CLAUDE.md.backup"
    fi
    
    # Run tick command
    if bash "$SCRIPTS_DIR/tick-command.sh" >/dev/null 2>&1; then
        if grep -q "You are currently in TICK STAGE" "$ROOT_DIR/CLAUDE.md"; then
            test_pass "tick command executed successfully"
        else
            test_fail "tick stage marker not found after command"
        fi
    else
        test_fail "tick command failed"
    fi
    
    # Restore backup if it existed
    if [[ -f "$TEMP_DIR/CLAUDE.md.backup" ]]; then
        mv "$TEMP_DIR/CLAUDE.md.backup" "$ROOT_DIR/CLAUDE.md"
    fi
}

# Test tock-command.sh
test_tock_command() {
    print_test_header "tock-command.sh"
    
    # Make backup of current CLAUDE.md if it exists
    if [[ -f "$ROOT_DIR/CLAUDE.md" ]]; then
        cp "$ROOT_DIR/CLAUDE.md" "$TEMP_DIR/CLAUDE.md.backup"
    fi
    
    # Run tock command
    if bash "$SCRIPTS_DIR/tock-command.sh" >/dev/null 2>&1; then
        if grep -q "You are currently in TOCK STAGE" "$ROOT_DIR/CLAUDE.md"; then
            test_pass "tock command executed successfully"
        else
            test_fail "tock stage marker not found after command"
        fi
    else
        test_fail "tock command failed"
    fi
    
    # Restore backup if it existed
    if [[ -f "$TEMP_DIR/CLAUDE.md.backup" ]]; then
        mv "$TEMP_DIR/CLAUDE.md.backup" "$ROOT_DIR/CLAUDE.md"
    fi
}

# Test workflow stage detection
test_workflow_stage_detection() {
    print_test_header "workflow stage detection"
    
    # Create test CLAUDE.md files
    echo "You are currently in TICK STAGE" > "$TEMP_DIR/CLAUDE_tick.md"
    echo "You are currently in TOCK STAGE" > "$TEMP_DIR/CLAUDE_tock.md"
    echo "No stage marker here" > "$TEMP_DIR/CLAUDE_none.md"
    
    # Test tick detection
    if grep -q "You are currently in TICK STAGE" "$TEMP_DIR/CLAUDE_tick.md"; then
        test_pass "tick stage detection pattern works"
    else
        test_fail "tick stage detection pattern failed"
    fi
    
    # Test tock detection
    if grep -q "You are currently in TOCK STAGE" "$TEMP_DIR/CLAUDE_tock.md"; then
        test_pass "tock stage detection pattern works"
    else
        test_fail "tock stage detection pattern failed"
    fi
    
    # Test no stage detection
    if ! grep -q "You are currently in TICK STAGE\|You are currently in TOCK STAGE" "$TEMP_DIR/CLAUDE_none.md"; then
        test_pass "no stage detection works"
    else
        test_fail "false positive stage detection"
    fi
}

# Test file permissions
test_file_permissions() {
    print_test_header "file permissions"
    
    # Check if scripts are executable
    if [[ -x "$SCRIPTS_DIR/tick-command.sh" ]]; then
        test_pass "tick-command.sh is executable"
    else
        test_fail "tick-command.sh is not executable"
    fi
    
    if [[ -x "$SCRIPTS_DIR/tock-command.sh" ]]; then
        test_pass "tock-command.sh is executable"
    else
        test_fail "tock-command.sh is not executable"
    fi
    
    if [[ -x "$SCRIPTS_DIR/generate-claude-md.py" ]]; then
        test_pass "generate-claude-md.py is executable"
    else
        test_fail "generate-claude-md.py is not executable"
    fi
}

# Test template files exist
test_template_files() {
    print_test_header "template files existence"
    
    if [[ -f "$ROOT_DIR/CLAUDE.md.template" ]]; then
        test_pass "CLAUDE.md.template exists"
    else
        test_fail "CLAUDE.md.template missing"
    fi
    
    if [[ -f "$ROOT_DIR/tick-tock-content/common-content.md" ]]; then
        test_pass "common-content.md exists"
    else
        test_fail "common-content.md missing"
    fi
    
    if [[ -f "$ROOT_DIR/tick-tock-content/tick-content.md" ]]; then
        test_pass "tick-content.md exists"
    else
        test_fail "tick-content.md missing"
    fi
    
    if [[ -f "$ROOT_DIR/tick-tock-content/tock-content.md" ]]; then
        test_pass "tock-content.md exists"
    else
        test_fail "tock-content.md missing"
    fi
}

# Main test execution
echo "==================================="
echo "Tick-Tock Methodology Test Suite"
echo "==================================="

# Run all tests
test_template_files
test_file_permissions
test_generate_claude_md_valid
test_generate_claude_md_invalid
test_tick_command
test_tock_command
test_workflow_stage_detection

# Print summary
echo -e "\n==================================="
echo "Test Summary"
echo "==================================="
echo "Tests run: $TESTS_RUN"
echo -e "Tests passed: ${GREEN}$TESTS_PASSED${NC}"
echo -e "Tests failed: ${RED}$TESTS_FAILED${NC}"

# Exit with appropriate code
if [[ $TESTS_FAILED -eq 0 ]]; then
    echo -e "\n${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "\n${RED}Some tests failed!${NC}"
    exit 1
fi