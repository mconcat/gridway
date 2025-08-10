#!/usr/bin/env python3
"""
Generate CLAUDE.md from template based on the current development stage.

This script reads the CLAUDE.md.template file and replaces placeholders with
stage-specific content to create a focused instruction set for the current
development phase.

Usage:
    python3 scripts/generate-claude-md.py tick
    python3 scripts/generate-claude-md.py tock
"""

import sys
import os
from pathlib import Path

def read_file(filepath):
    """Read file content safely."""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            return f.read()
    except FileNotFoundError:
        print(f"Error: File {filepath} not found")
        sys.exit(1)
    except Exception as e:
        print(f"Error reading {filepath}: {e}")
        sys.exit(1)

def write_file(filepath, content):
    """Write file content safely."""
    try:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"Successfully generated {filepath}")
    except Exception as e:
        print(f"Error writing {filepath}: {e}")
        sys.exit(1)

def generate_claude_md(stage):
    """Generate CLAUDE.md for the specified stage."""
    # Validate stage input to prevent any injection
    valid_stages = ['tick', 'tock']
    if stage not in valid_stages:
        print(f"Error: Stage must be one of: {', '.join(valid_stages)}")
        sys.exit(1)
    
    # Additional validation to ensure no path traversal characters
    if any(char in stage for char in ['/', '\\', '..', '~']):
        print("Error: Invalid characters in stage name")
        sys.exit(1)
    
    # Get the root directory (where this script is located)
    root_dir = Path(__file__).parent.parent
    
    # Read template file
    template_path = root_dir / 'CLAUDE.md.template'
    template_content = read_file(template_path)
    
    # Read common content
    common_content_path = root_dir / 'tick-tock-content' / 'common-content.md'
    common_content = read_file(common_content_path)
    
    # Read stage-specific content
    stage_content_path = root_dir / 'tick-tock-content' / f'{stage}-content.md'
    stage_content = read_file(stage_content_path)
    
    # Replace placeholders
    result = template_content.replace('{{COMMON_CONTENT}}', common_content)
    result = result.replace('{{STAGE_CONTENT}}', stage_content)
    
    # Write the result to CLAUDE.md
    output_path = root_dir / 'CLAUDE.md'
    write_file(output_path, result)
    
    print(f"Generated CLAUDE.md for {stage.upper()} stage")

def main():
    if len(sys.argv) != 2:
        print("Usage: python3 scripts/generate-claude-md.py <stage>")
        print("       where <stage> is 'tick' or 'tock'")
        sys.exit(1)
    
    stage = sys.argv[1].lower().strip()
    
    # Sanitize input to prevent any malicious input
    if len(stage) > 10:  # stage names should be short
        print("Error: Stage name too long")
        sys.exit(1)
    generate_claude_md(stage)

if __name__ == '__main__':
    main()