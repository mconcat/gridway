#!/usr/bin/env python3
"""
Script to fix TxResponse creations in Rust code by adding missing fields.

The script performs these transformations:
1. Add "data: vec![]," after "code: X,"
2. Add "info: String::new()," after "log: ..."
3. Convert gas_wanted and gas_used to i64 by adding "as i64"
4. Add "codespace: String::new()," after "events: ..."
"""

import re
import sys
import os

def fix_txresponse_in_content(content, start_line=670):
    """Fix TxResponse creations in the content starting from a specific line."""
    lines = content.split('\n')
    
    # Process lines starting from start_line
    i = start_line - 1  # Convert to 0-based index
    modified = False
    
    while i < len(lines):
        line = lines[i]
        
        # Check if this line contains "TxResponse {"
        if "TxResponse {" in line:
            # We found a TxResponse creation, now process it
            j = i + 1
            brace_count = 1
            txresponse_lines = [line]
            
            # Collect all lines until the closing brace
            while j < len(lines) and brace_count > 0:
                current_line = lines[j]
                txresponse_lines.append(current_line)
                
                # Count braces to find the end of TxResponse
                brace_count += current_line.count('{') - current_line.count('}')
                j += 1
            
            # Now process the collected TxResponse lines
            fixed_lines = fix_txresponse_fields(txresponse_lines)
            
            if fixed_lines != txresponse_lines:
                modified = True
                # Replace the original lines with fixed ones
                for k, fixed_line in enumerate(fixed_lines):
                    if i + k < len(lines):
                        lines[i + k] = fixed_line
            
            # Move to the line after this TxResponse
            i = j
        else:
            i += 1
    
    return '\n'.join(lines), modified

def fix_txresponse_fields(txresponse_lines):
    """Fix the fields in a TxResponse creation."""
    fixed_lines = []
    
    # Flags to track what we've already added
    has_data = False
    has_info = False
    has_codespace = False
    
    # Check what fields already exist
    full_text = '\n'.join(txresponse_lines)
    if re.search(r'\bdata\s*:', full_text):
        has_data = True
    if re.search(r'\binfo\s*:', full_text):
        has_info = True
    if re.search(r'\bcodespace\s*:', full_text):
        has_codespace = True
    
    for i, line in enumerate(txresponse_lines):
        # Fix gas_wanted and gas_used to add "as i64" if needed
        if re.search(r'\bgas_wanted\s*:\s*\d+(?!\s*as\s*i64)', line):
            line = re.sub(r'(\bgas_wanted\s*:\s*)(\d+)', r'\1\2 as i64', line)
        elif re.search(r'\bgas_wanted\s*:\s*[^,\s]+(?!\s*as\s*i64)(?=\s*,)', line):
            # Handle expressions like total_gas_used + 10000
            line = re.sub(r'(\bgas_wanted\s*:\s*)([^,]+)', r'\1(\2) as i64', line)
        
        if re.search(r'\bgas_used\s*:\s*\d+(?!\s*as\s*i64)', line):
            line = re.sub(r'(\bgas_used\s*:\s*)(\d+)', r'\1\2 as i64', line)
        elif re.search(r'\bgas_used\s*:\s*[^,\s]+(?!\s*as\s*i64)(?=\s*,)', line):
            # Handle variables like total_gas_used
            line = re.sub(r'(\bgas_used\s*:\s*)([^,]+)', r'\1\2 as i64', line)
        
        fixed_lines.append(line)
        
        # Add "data: vec![]," after "code: X," if not already present
        if not has_data and re.search(r'\bcode\s*:\s*\d+\s*,', line):
            indent = get_indent(line)
            fixed_lines.append(f"{indent}data: vec![],")
            has_data = True
        
        # Add "info: String::new()," after "log: ..." if not already present
        if not has_info and re.search(r'\blog\s*:.*,', line):
            indent = get_indent(line)
            fixed_lines.append(f"{indent}info: String::new(),")
            has_info = True
        
        # Add "codespace: String::new()," after "events: ..." if not already present
        if not has_codespace and re.search(r'\bevents\s*:.*,', line):
            # Check if this is the last field (no comma after the closing bracket/paren)
            if i + 1 < len(txresponse_lines):
                next_line = txresponse_lines[i + 1].strip()
                if next_line.startswith('}') or next_line.startswith('})'):
                    # This is the last field, add comma to events line if needed
                    if not line.rstrip().endswith(','):
                        fixed_lines[-1] = line.rstrip() + ','
                    indent = get_indent(line)
                    fixed_lines.append(f"{indent}codespace: String::new(),")
                    has_codespace = True
    
    return fixed_lines

def get_indent(line):
    """Extract the indentation from a line."""
    return line[:len(line) - len(line.lstrip())]

def main():
    if len(sys.argv) != 2:
        print("Usage: python fix_txresponse.py <file_path>")
        sys.exit(1)
    
    file_path = sys.argv[1]
    
    if not os.path.exists(file_path):
        print(f"Error: File '{file_path}' not found")
        sys.exit(1)
    
    # Read the file
    with open(file_path, 'r') as f:
        content = f.read()
    
    # Fix TxResponse creations
    fixed_content, modified = fix_txresponse_in_content(content, start_line=670)
    
    if modified:
        # Create backup
        backup_path = file_path + '.backup'
        with open(backup_path, 'w') as f:
            f.write(content)
        print(f"Created backup at: {backup_path}")
        
        # Write the fixed content
        with open(file_path, 'w') as f:
            f.write(fixed_content)
        print(f"Fixed TxResponse creations in: {file_path}")
        print("Changes applied successfully!")
    else:
        print("No changes needed - all TxResponse creations appear to be correct.")

if __name__ == "__main__":
    main()