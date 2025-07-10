#!/bin/bash

# Build script for all WASI modules
# This compiles all Rust WASI modules to WebAssembly

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Building WASI modules..."

# Install wasm32-wasi target if not already installed
if ! rustup target list --installed | grep -q wasm32-wasip1; then
    echo "Installing wasm32-wasip1 target..."
    rustup target add wasm32-wasip1
fi

# Create modules directory
MODULES_DIR="$PROJECT_ROOT/modules"
mkdir -p "$MODULES_DIR"

# List of modules to build
MODULES=(
    "ante-handler:wasi_ante_handler:ante_handler"
    "begin-blocker:begin_blocker:begin_blocker"
    "end-blocker:end_blocker:end_blocker"
    "tx-decoder:tx_decoder:tx_decoder"
    "test-minimal:test_minimal:test_minimal"
)

# Build each module
for module_info in "${MODULES[@]}"; do
    IFS=':' read -r module_name crate_name output_name <<< "$module_info"
    
    echo "Building $module_name..."
    
    cd "$PROJECT_ROOT/crates/wasi-modules/$module_name"
    
    # Build the module as a component
    cargo component build --release
    
    # Copy the built component
    WASM_FILE="$PROJECT_ROOT/target/wasm32-wasip1/release/${crate_name}.wasm"
    LIB_WASM_FILE="$PROJECT_ROOT/target/wasm32-wasip1/release/lib${crate_name}.wasm"
    
    if [ -f "$WASM_FILE" ]; then
        cp "$WASM_FILE" "$MODULES_DIR/${output_name}_component.wasm"
        echo "✓ Copied $module_name to $MODULES_DIR/${output_name}_component.wasm"
    elif [ -f "$LIB_WASM_FILE" ]; then
        cp "$LIB_WASM_FILE" "$MODULES_DIR/${output_name}_component.wasm"
        echo "✓ Copied $module_name to $MODULES_DIR/${output_name}_component.wasm"
    else
        echo "✗ Warning: Could not find compiled WASM file for $module_name"
    fi
done

echo ""
echo "WASI modules build complete!"
echo "Modules are located in: $MODULES_DIR"
ls -la "$MODULES_DIR"/*.wasm 2>/dev/null || echo "No modules found"