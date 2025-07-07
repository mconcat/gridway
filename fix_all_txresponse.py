#!/usr/bin/env python3
import re
import sys

def fix_txresponse_in_file(filename):
    with open(filename, 'r') as f:
        content = f.read()
    
    lines = content.split('\n')
    result = []
    i = 0
    
    while i < len(lines):
        line = lines[i]
        
        # Check if this is a TxResponse creation
        if 'TxResponse {' in line and i > 0:
            # Look for the pattern of TxResponse creation
            j = i + 1
            tx_response_lines = [line]
            brace_count = 1
            has_data = False
            has_info = False
            has_codespace = False
            
            while j < len(lines) and brace_count > 0:
                tx_line = lines[j]
                
                # Track braces
                brace_count += tx_line.count('{') - tx_line.count('}')
                
                # Check what fields we already have
                if 'data:' in tx_line:
                    has_data = True
                if 'info:' in tx_line:
                    has_info = True
                if 'codespace:' in tx_line:
                    has_codespace = True
                
                # Add missing fields in the right places
                if 'code:' in tx_line and not has_data:
                    tx_response_lines.append(tx_line)
                    # Add data field
                    indent = ' ' * (len(tx_line) - len(tx_line.lstrip()))
                    tx_response_lines.append(f'{indent}data: vec![],')
                    has_data = True
                elif 'log:' in tx_line and not has_info:
                    tx_response_lines.append(tx_line)
                    # Add info field
                    indent = ' ' * (len(tx_line) - len(tx_line.lstrip()))
                    tx_response_lines.append(f'{indent}info: String::new(),')
                    has_info = True
                elif 'gas_wanted:' in tx_line and ' as i64' not in tx_line:
                    # Fix gas_wanted to i64
                    if tx_line.rstrip().endswith(','):
                        tx_line = tx_line.rstrip()[:-1] + ' as i64,'
                    else:
                        tx_line = tx_line.rstrip() + ' as i64'
                    tx_response_lines.append(tx_line)
                elif 'gas_used:' in tx_line and ' as i64' not in tx_line:
                    # Fix gas_used to i64
                    if tx_line.rstrip().endswith(','):
                        tx_line = tx_line.rstrip()[:-1] + ' as i64,'
                    else:
                        tx_line = tx_line.rstrip() + ' as i64'
                    tx_response_lines.append(tx_line)
                elif 'events:' in tx_line and not has_codespace and brace_count == 1:
                    tx_response_lines.append(tx_line)
                    # Add codespace field
                    indent = ' ' * (len(tx_line) - len(tx_line.lstrip()))
                    tx_response_lines.append(f'{indent}codespace: String::new(),')
                    has_codespace = True
                else:
                    tx_response_lines.append(tx_line)
                
                j += 1
            
            # Add the processed TxResponse to result
            result.extend(tx_response_lines[1:])  # Skip the first line as it was already added
            i = j
        else:
            result.append(line)
            i += 1
    
    # Write back
    with open(filename, 'w') as f:
        f.write('\n'.join(result))

if __name__ == '__main__':
    if len(sys.argv) != 2:
        print("Usage: python fix_all_txresponse.py <filename>")
        sys.exit(1)
    
    fix_txresponse_in_file(sys.argv[1])
    print(f"Fixed all TxResponse creations in {sys.argv[1]}")