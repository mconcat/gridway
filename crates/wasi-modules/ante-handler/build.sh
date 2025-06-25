#!/bin/bash

# Build script for WASI ante handler module
# This compiles the Rust code to WebAssembly with WASI support

set -e

echo "Building WASI ante handler module..."

# Install wasm32-wasi target if not already installed
rustup target add wasm32-wasi

# Build the WASM module
cargo build --target wasm32-wasi --release

# Copy the built module to a known location
OUTPUT_DIR="../../../target/wasm32-wasi/release"
MODULE_DIR="../../../modules"

mkdir -p "$MODULE_DIR"
cp "$OUTPUT_DIR/wasi_ante_handler.wasm" "$MODULE_DIR/ante_handler.wasm" 2>/dev/null || \
cp "$OUTPUT_DIR/libwasi_ante_handler.wasm" "$MODULE_DIR/ante_handler.wasm" 2>/dev/null || \
echo "Warning: Could not find compiled WASM file"

echo "WASI ante handler module built successfully!"
echo "Output: $MODULE_DIR/ante_handler.wasm"