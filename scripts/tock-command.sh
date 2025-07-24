#!/bin/bash
# Tock command handler for Claude Code custom slash commands
# This script switches the development methodology to TOCK stage

set -euo pipefail  # Exit on error, undefined variables, and pipe failures

echo "📚 Switching to TOCK stage (Architectural Refinement)"
echo "====================================================="

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

# Generate CLAUDE.md for tock stage
if ! python3 "$SCRIPT_DIR/generate-claude-md.py" tock; then
    echo "Error: Failed to generate CLAUDE.md for tock stage"
    exit 1
fi

# Verify the file was created successfully
if [[ ! -f "$ROOT_DIR/CLAUDE.md" ]]; then
    echo "Error: CLAUDE.md was not created"
    exit 1
fi

# Verify it contains tock stage marker
if ! grep -q "You are currently in TOCK STAGE" "$ROOT_DIR/CLAUDE.md"; then
    echo "Warning: CLAUDE.md may not have been updated correctly"
fi

echo ""
echo "✅ Successfully switched to TOCK stage"
echo ""
echo "📋 TOCK Stage Guidelines:"
echo "  • Documentation First: Comprehensive documentation is priority"
echo "  • Architectural Clarity: Focus on system design and interfaces"
echo "  • Code Hygiene: Refactor and clean up existing code"
echo "  • Conversational Mode: Actively seek user input and dialogue"
echo "  • Less Agentic: Work collaboratively rather than autonomously"
echo ""
echo "✅ PERMITTED in TOCK:"
echo "  • TODOs and FIXMEs for architectural planning"
echo "  • Mock implementations for architectural backbone"
echo "  • Temporary build failures during refactoring"
echo ""
echo "🎯 Ready for architectural refinement and documentation!"