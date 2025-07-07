#!/usr/bin/env python3
import re

def fix_txresponse_file(filename):
    with open(filename, 'r') as f:
        content = f.read()
    
    # Fix all TxResponse creations
    # Pattern to match TxResponse { ... }
    pattern = r'((?:return )?Ok\(TxResponse \{)((?:[^{}]|\{[^{}]*\})*)\}'
    
    def fix_txresponse(match):
        prefix = match.group(1)
        fields_str = match.group(2)
        
        # Parse existing fields
        fields = {}
        # Match field: value pairs, handling multi-line values
        field_pattern = r'(\w+):\s*([^,]+(?:\([^)]*\)[^,]*)?(?:\{[^}]*\}[^,]*)?)'
        for field_match in re.finditer(field_pattern, fields_str):
            field_name = field_match.group(1).strip()
            field_value = field_match.group(2).strip().rstrip(',')
            fields[field_name] = field_value
        
        # Build new TxResponse with all required fields
        result = prefix + '\n'
        
        # Add fields in order
        result += f'            code: {fields.get("code", "0")},\n'
        result += '            data: vec![],\n'
        result += f'            log: {fields.get("log", "String::new()")},\n'
        result += '            info: String::new(),\n'
        
        # Fix gas_wanted - add as i64 if not present
        gas_wanted = fields.get("gas_wanted", "0")
        if ' as i64' not in gas_wanted:
            gas_wanted = gas_wanted.rstrip() + ' as i64'
        result += f'            gas_wanted: {gas_wanted},\n'
        
        # Fix gas_used - add as i64 if not present
        gas_used = fields.get("gas_used", "0")
        if ' as i64' not in gas_used:
            gas_used = gas_used.rstrip() + ' as i64'
        result += f'            gas_used: {gas_used},\n'
        
        result += f'            events: {fields.get("events", "vec![]")},\n'
        result += '            codespace: String::new(),\n'
        result += '        }'
        
        return result
    
    # Apply fixes
    content = re.sub(pattern, fix_txresponse, content, flags=re.DOTALL)
    
    # Write back
    with open(filename, 'w') as f:
        f.write(content)

if __name__ == '__main__':
    fix_txresponse_file('crates/helium-baseapp/src/lib.rs')
    print("Fixed all TxResponse instances")