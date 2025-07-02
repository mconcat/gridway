.PHONY: all setup install-deps build test clean run dev check fmt

# Default target
all: setup build

# Setup development environment
setup: install-deps
	@echo "✓ Development environment setup complete"

# Install all dependencies
install-deps: check-rust install-system-deps
	@echo "Installing Rust dependencies..."
	cargo fetch
	@echo "✓ All dependencies installed"

# Check if Rust is installed
check-rust:
	@command -v rustc >/dev/null 2>&1 || { echo "Error: Rust is not installed. Please install from https://rustup.rs/"; exit 1; }
	@command -v cargo >/dev/null 2>&1 || { echo "Error: Cargo is not installed. Please install from https://rustup.rs/"; exit 1; }
	@echo "✓ Rust toolchain found: $$(rustc --version)"

# Install system dependencies (platform-specific)
install-system-deps:
	@echo "Checking system dependencies..."
	@if [ "$$(uname)" = "Darwin" ]; then \
		echo "macOS detected. Checking dependencies..."; \
		command -v brew >/dev/null 2>&1 || { echo "Error: Homebrew is not installed. Please install from https://brew.sh/"; exit 1; }; \
		brew list rocksdb >/dev/null 2>&1 || { echo "Installing RocksDB..."; brew install rocksdb; }; \
		brew list protobuf >/dev/null 2>&1 || { echo "Installing Protobuf..."; brew install protobuf; }; \
	elif [ "$$(uname)" = "Linux" ]; then \
		echo "Linux detected. Please ensure the following are installed:"; \
		echo "  - librocksdb-dev (Ubuntu/Debian: sudo apt-get install librocksdb-dev)"; \
		echo "  - protobuf-compiler (Ubuntu/Debian: sudo apt-get install protobuf-compiler)"; \
		echo "  - libssl-dev (Ubuntu/Debian: sudo apt-get install libssl-dev)"; \
		echo "  - pkg-config (Ubuntu/Debian: sudo apt-get install pkg-config)"; \
	else \
		echo "Warning: Unknown operating system. Please manually install:"; \
		echo "  - RocksDB development libraries"; \
		echo "  - Protocol Buffers compiler"; \
	fi
	@echo "✓ System dependency check complete"

# Build the project
build:
	@echo "Building Helium..."
	cargo build --release
	@echo "✓ Build complete"

# Build in debug mode
build-debug:
	@echo "Building Helium (debug mode)..."
	cargo build
	@echo "✓ Debug build complete"

# Run tests
test:
	@echo "Running tests..."
	cargo test --all
	@echo "✓ All tests passed"

# Run with verbose test output
test-verbose:
	cargo test --all -- --nocapture

# Run the application
run:
	cargo run --release

# Development mode with auto-reload (requires cargo-watch)
dev:
	@command -v cargo-watch >/dev/null 2>&1 || { echo "Installing cargo-watch..."; cargo install cargo-watch; }
	cargo watch -x "run"

# Check code (without building)
check:
	@echo "Checking code..."
	cargo check --all
	@echo "✓ Code check complete"

# Format code
fmt:
	@echo "Formatting code..."
	cargo fmt --all
	@echo "✓ Code formatted"

# Lint code
lint:
	@echo "Linting code..."
	cargo clippy --all -- -D warnings
	@echo "✓ Linting complete"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	@echo "✓ Clean complete"

# Install additional development tools
install-dev-tools:
	@echo "Installing development tools..."
	cargo install cargo-watch || true
	cargo install cargo-audit || true
	cargo install cargo-outdated || true
	@echo "✓ Development tools installed"

# Security audit
audit:
	@command -v cargo-audit >/dev/null 2>&1 || { echo "Installing cargo-audit..."; cargo install cargo-audit; }
	cargo audit

# Check for outdated dependencies
outdated:
	@command -v cargo-outdated >/dev/null 2>&1 || { echo "Installing cargo-outdated..."; cargo install cargo-outdated; }
	cargo outdated

# Build WASM modules
build-wasm:
	@echo "Building WASM modules..."
	cd crates/wasi-modules && cargo build --release --target wasm32-wasip1
	@echo "✓ WASM modules built"

# Quick setup for CI/CD environments
ci-setup:
	rustup component add rustfmt clippy
	cargo fetch

# Help
help:
	@echo "Helium Development Makefile"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Main targets:"
	@echo "  setup          - Set up complete development environment"
	@echo "  build          - Build the project in release mode"
	@echo "  test           - Run all tests"
	@echo "  run            - Run the application"
	@echo "  clean          - Clean build artifacts"
	@echo ""
	@echo "Development targets:"
	@echo "  build-debug    - Build in debug mode"
	@echo "  dev            - Run in development mode with auto-reload"
	@echo "  check          - Check code without building"
	@echo "  fmt            - Format code"
	@echo "  lint           - Run clippy linter"
	@echo "  test-verbose   - Run tests with verbose output"
	@echo ""
	@echo "Additional targets:"
	@echo "  install-deps      - Install all dependencies"
	@echo "  install-dev-tools - Install additional development tools"
	@echo "  audit            - Run security audit"
	@echo "  outdated         - Check for outdated dependencies"
	@echo "  build-wasm       - Build WASM modules"
	@echo "  ci-setup         - Quick setup for CI environments"