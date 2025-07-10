# Build stage
FROM rust:1.75-slim AS builder

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /usr/src/helium

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build the application
RUN cargo build --release --bin helium-server

# Build WASI modules
RUN cargo build --release -p ante-handler --target wasm32-wasi
RUN cargo build --release -p begin-blocker --target wasm32-wasi
RUN cargo build --release -p end-blocker --target wasm32-wasi
RUN cargo build --release -p tx-decoder --target wasm32-wasi

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    netcat-traditional \
    && rm -rf /var/lib/apt/lists/*

# Create helium user
RUN useradd -m -u 1000 -s /bin/bash helium

# Copy binary from builder
COPY --from=builder /usr/src/helium/target/release/helium-server /usr/local/bin/helium-server

# Copy WASI modules
RUN mkdir -p /usr/local/lib/helium/wasi-modules
COPY --from=builder /usr/src/helium/target/wasm32-wasi/release/*.wasm /usr/local/lib/helium/wasi-modules/

# Create data directory
RUN mkdir -p /helium && chown helium:helium /helium

# Switch to helium user
USER helium

# Set working directory
WORKDIR /helium

# Expose ports
EXPOSE 26658 9090 1317

# Set environment variables
ENV RUST_LOG=info
ENV HELIUM_HOME=/helium
ENV WASI_MODULE_PATH=/usr/local/lib/helium/wasi-modules

# Default command
CMD ["helium-server", "start", "--home", "/helium"]