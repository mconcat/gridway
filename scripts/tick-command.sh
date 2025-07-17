#!/bin/bash
# Tick command handler for Claude Code custom slash commands
# This script switches the development methodology to TICK stage

set -e

echo "🚀 Switching to TICK stage (Implementation Velocity)"
echo "================================================="

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# Generate CLAUDE.md for tick stage
python3 "$SCRIPT_DIR/generate-claude-md.py" tick

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