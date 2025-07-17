#!/bin/bash
# Tock command handler for Claude Code custom slash commands
# This script switches the development methodology to TOCK stage

set -e

echo "📚 Switching to TOCK stage (Architectural Refinement)"
echo "====================================================="

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# Generate CLAUDE.md for tock stage
python3 "$SCRIPT_DIR/generate-claude-md.py" tock

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