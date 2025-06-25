#!/bin/bash

# Build script for WASI TxDecoder module

set -e

echo "Building TxDecoder WASI module..."

# Ensure we have the wasm32-wasi target
rustup target add wasm32-wasi

# Build the module
cargo build --target wasm32-wasi --release

# Copy the built module to a standard location
mkdir -p ../../../modules
cp target/wasm32-wasi/release/tx_decoder.wasm ../../../modules/tx_decoder.wasm

echo "TxDecoder WASI module built successfully!"
echo "Output: modules/tx_decoder.wasm"

# Optional: Optimize the WASM module size with wasm-opt if available
if command -v wasm-opt &> /dev/null; then
    echo "Optimizing WASM module with wasm-opt..."
    wasm-opt -Oz ../../../modules/tx_decoder.wasm -o ../../../modules/tx_decoder_opt.wasm
    mv ../../../modules/tx_decoder_opt.wasm ../../../modules/tx_decoder.wasm
    echo "WASM module optimized!"
fi

# Show module info
ls -lh ../../../modules/tx_decoder.wasm