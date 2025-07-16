#!/bin/bash

# Build script for WASI EndBlock module

set -e

echo "Building EndBlock WASI module..."

# Ensure we have the wasm32-wasip1 target
rustup target add wasm32-wasip1

# Build the module
cargo build --target wasm32-wasip1 --release

# Copy the built module to a standard location
mkdir -p ../../../modules
cp target/wasm32-wasip1/release/end_blocker.wasm ../../../modules/end_blocker.wasm

echo "EndBlock WASI module built successfully!"
echo "Output: modules/end_blocker.wasm"

# Optional: Optimize the WASM module size with wasm-opt if available
if command -v wasm-opt &> /dev/null; then
    echo "Optimizing WASM module with wasm-opt..."
    wasm-opt -Oz ../../../modules/end_blocker.wasm -o ../../../modules/end_blocker_opt.wasm
    mv ../../../modules/end_blocker_opt.wasm ../../../modules/end_blocker.wasm
    echo "WASM module optimized!"
fi

# Show module info
ls -lh ../../../modules/end_blocker.wasm