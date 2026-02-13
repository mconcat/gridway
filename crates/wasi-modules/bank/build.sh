#!/bin/bash
# Build script for WASI Bank module (Component Model)

set -e

echo "Building Bank WASI component..."

# Ensure we have the wasm32-wasip2 target
rustup target add wasm32-wasip2 2>/dev/null || true

# Build with cargo-component (component model)
if command -v cargo-component &> /dev/null; then
    echo "Using cargo-component..."
    cargo component build --release
    mkdir -p ../../../modules
    cp target/wasm32-wasip2/release/bank.wasm ../../../modules/bank_component.wasm
else
    echo "cargo-component not found. Install with: cargo install cargo-component"
    echo "Falling back to plain cargo build..."
    cargo build --target wasm32-wasip2 --release
    mkdir -p ../../../modules
    cp target/wasm32-wasip2/release/bank.wasm ../../../modules/bank_component.wasm
fi

echo "Bank WASI component built successfully!"
echo "Output: modules/bank_component.wasm"
ls -lh ../../../modules/bank_component.wasm
