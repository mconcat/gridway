#!/bin/bash

# Fix all remaining TxResponse creations
file="crates/helium-baseapp/src/lib.rs"

# Function to fix a TxResponse at a given line
fix_txresponse() {
    local start_line=$1
    local temp_file=$(mktemp)
    
    # Read the file and process
    awk -v start=$start_line '
    NR < start { print; next }
    /Ok\(TxResponse \{/ && !done {
        in_response = 1
        print
        next
    }
    in_response {
        # Add data field after code
        if (/code:/ && !has_data) {
            print
            print "            data: vec![],"
            has_data = 1
            next
        }
        # Add info field after log
        if (/log:/ && !has_info) {
            print
            print "            info: String::new(),"
            has_info = 1
            next
        }
        # Convert gas_wanted to i64
        if (/gas_wanted:/ && !/as i64/) {
            gsub(/,$/, " as i64,")
            print
            next
        }
        # Convert gas_used to i64
        if (/gas_used:/ && !/as i64/) {
            gsub(/,$/, " as i64,")
            print
            next
        }
        # Add codespace after events
        if (/events:/ && !has_codespace) {
            print
            print "            codespace: String::new(),"
            has_codespace = 1
            next
        }
        # End of TxResponse
        if (/}\)/) {
            in_response = 0
            has_data = 0
            has_info = 0
            has_codespace = 0
            done = 1
        }
        print
        next
    }
    { print }
    ' "$file" > "$temp_file"
    
    mv "$temp_file" "$file"
}

# Fix each remaining TxResponse (starting from line 670)
for line in 673 831 890 926 962 989 1000 1076; do
    echo "Fixing TxResponse at line ~$line"
    fix_txresponse $((line - 3))
done

echo "All TxResponse creations fixed!"