# Build stage
FROM rustlang/rust:nightly-slim AS builder

# Install dependencies (no protobuf needed — pure Rust consensus)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl \
    build-essential \
    clang \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Add rustfmt component
RUN rustup component add rustfmt

# Create app directory
WORKDIR /usr/src/gridway

# Copy workspace files
COPY . ./

# Build all three binaries from gridway-consensus crate
RUN cargo build --release -p gridway-consensus \
    --bin gridway-node \
    --bin gridway-setup \
    --bin gridway-keygen

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create gridway user
RUN useradd -m -u 1000 -s /bin/bash gridway

# Copy binaries from builder
COPY --from=builder /usr/src/gridway/target/release/gridway-node /usr/local/bin/gridway-node
COPY --from=builder /usr/src/gridway/target/release/gridway-setup /usr/local/bin/gridway-setup
COPY --from=builder /usr/src/gridway/target/release/gridway-keygen /usr/local/bin/gridway-keygen

# Copy pre-compiled WASM modules
RUN mkdir -p /usr/local/lib/gridway/wasi-modules
COPY modules/*.wasm /usr/local/lib/gridway/wasi-modules/

# Create data directory
RUN mkdir -p /gridway && chown gridway:gridway /gridway

# Switch to gridway user
USER gridway

# Set working directory
WORKDIR /gridway

# Expose ports: P2P, metrics, HTTP API
EXPOSE 4545 4546 4547

# Set environment variables
ENV RUST_LOG=info
ENV WASI_MODULE_PATH=/usr/local/lib/gridway/wasi-modules

# Entrypoint
ENTRYPOINT ["gridway-node"]
CMD ["--peers=/gridway/peers.yaml", "--config=/gridway/config.yaml", "--genesis=/gridway/genesis.yaml"]
