#!/bin/bash

# Build script for WASI BeginBlock module

set -e

echo "Building BeginBlock WASI module..."

# Ensure we have the wasm32-wasip1 target
rustup target add wasm32-wasip1

# Build the module
cargo build --target wasm32-wasip1 --release

# Copy the built module to a standard location
mkdir -p ../../../modules
cp target/wasm32-wasip1/release/begin_blocker.wasm ../../../modules/begin_blocker.wasm

echo "BeginBlock WASI module built successfully!"
echo "Output: modules/begin_blocker.wasm"

# Optional: Optimize the WASM module size with wasm-opt if available
if command -v wasm-opt &> /dev/null; then
    echo "Optimizing WASM module with wasm-opt..."
    wasm-opt -Oz ../../../modules/begin_blocker.wasm -o ../../../modules/begin_blocker_opt.wasm
    mv ../../../modules/begin_blocker_opt.wasm ../../../modules/begin_blocker.wasm
    echo "WASM module optimized!"
fi

# Show module info
ls -lh ../../../modules/begin_blocker.wasm