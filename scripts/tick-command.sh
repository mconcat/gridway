#!/bin/bash
# Tick command handler for Claude Code custom slash commands
# This script switches the development methodology to TICK stage

set -euo pipefail  # Exit on error, undefined variables, and pipe failures

echo "🚀 Switching to TICK stage (Implementation Velocity)"
echo "================================================="

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# Verify we're in the correct directory structure
if [[ ! -f "$ROOT_DIR/CLAUDE.md.template" ]]; then
    echo "Error: Cannot find CLAUDE.md.template in expected location"
    echo "Please run this script from the project root or scripts directory"
    exit 1
fi

# Check if Python 3 is available
if ! command -v python3 &> /dev/null; then
    echo "Error: Python 3 is required but not installed"
    exit 1
fi

# Generate CLAUDE.md for tick stage
if ! python3 "$SCRIPT_DIR/generate-claude-md.py" tick; then
    echo "Error: Failed to generate CLAUDE.md for tick stage"
    exit 1
fi

# Verify the file was created successfully
if [[ ! -f "$ROOT_DIR/CLAUDE.md" ]]; then
    echo "Error: CLAUDE.md was not created"
    exit 1
fi

# Verify it contains tick stage marker
if ! grep -q "You are currently in TICK STAGE" "$ROOT_DIR/CLAUDE.md"; then
    echo "Warning: CLAUDE.md may not have been updated correctly"
fi

echo ""
echo "✅ Successfully switched to TICK stage"
echo ""
echo "📋 TICK Stage Guidelines:"
echo "  • Maximum Speed: Prioritize working code over perfect code"
echo "  • Fast Merges: Merge changes as quickly as possible"
echo "  • Security First: Security checks are NEVER omitted"
echo "  • Activity Documentation: Record all work activities"
echo "  • Autopilot Mode: Work with minimal user interaction"
echo ""
echo "🚫 STRICTLY PROHIBITED in TICK:"
echo "  • TODOs, FIXMEs, or XXX comments"
echo "  • Mock implementations (todo!(), unimplemented!())"
echo "  • Broken builds or failing tests"
echo ""
echo "⚡ Ready for high-velocity development!"