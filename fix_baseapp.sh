#!/bin/bash

# File to fix
FILE="crates/helium-baseapp/src/lib.rs"

# Backup
cp "$FILE" "$FILE.bak"

# Fix 1: Replace event_type with r#type
sed -i '' 's/event_type: /r#type: /g' "$FILE"

# Fix 2: Replace all Attribute { with EventAttribute { and add index field
perl -i -pe 's/(\s+)Attribute \{/$1EventAttribute {/g' "$FILE"
perl -i -0pe 's/(EventAttribute \{[^}]*?value: [^,]+),(\s*\})/$1,\n$2            index: true,$2/g' "$FILE"

# Fix 3: Fix TxResponse creations - add missing fields
# This is complex, so we'll do it with a more sophisticated approach
perl -i -0pe '
s/((?:return )?Ok\(TxResponse \{)([^}]+)\}/
my $prefix = $1;
my $content = $2;
my $result = $prefix;

# Parse existing fields
my %fields;
while ($content =~ /(\w+):\s*([^,]+(?:,[^,]+)*),?/g) {
    $fields{$1} = $2;
}

# Add missing fields in order
$result .= "\n            code: " . ($fields{code} || "0") . ",";
$result .= "\n            data: vec![],";
$result .= "\n            log: " . ($fields{log} || "String::new()") . ",";
$result .= "\n            info: String::new(),";

# Convert gas fields to i64
if ($fields{gas_wanted}) {
    my $gas = $fields{gas_wanted};
    $gas =~ s/\s*$//;
    if ($gas !~ /as i64/) {
        $gas .= " as i64";
    }
    $result .= "\n            gas_wanted: $gas,";
} else {
    $result .= "\n            gas_wanted: 0,";
}

if ($fields{gas_used}) {
    my $gas = $fields{gas_used};
    $gas =~ s/\s*$//;
    if ($gas !~ /as i64/) {
        $gas .= " as i64";
    }
    $result .= "\n            gas_used: $gas,";
} else {
    $result .= "\n            gas_used: 0,";
}

$result .= "\n            events: " . ($fields{events} || "vec![]") . ",";
$result .= "\n            codespace: String::new(),";
$result .= "\n        }";
$result/ges;
' "$FILE"

echo "Fixed all issues in $FILE"