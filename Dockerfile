# Build stage
FROM rust:1.82-slim AS builder

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    curl \
    build-essential \
    clang \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-component (use older version compatible with edition 2021)
RUN cargo install cargo-component --version 0.17.0 --locked

# Add wasm32-wasip1 target and rustfmt component
RUN rustup target add wasm32-wasip1 && \
    rustup component add rustfmt

# Create app directory
WORKDIR /usr/src/helium

# Copy workspace files
COPY . ./

# Build WASI modules first using cargo-component
RUN ./scripts/build-wasi-modules.sh

# Build the application (now that WASI bindings exist)
RUN cargo build --release -p helium-server --bin helium-server

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
COPY --from=builder /usr/src/helium/modules/*.wasm /usr/local/lib/helium/wasi-modules/

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