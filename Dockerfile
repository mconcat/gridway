# Build stage
FROM rustlang/rust:nightly-slim AS builder

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

# Install cargo-component
RUN cargo install cargo-component --locked

# Add wasm32-wasip1 target and rustfmt component
RUN rustup target add wasm32-wasip1 && \
    rustup component add rustfmt

# Create app directory
WORKDIR /usr/src/gridway

# Copy workspace files
COPY . ./

# Build WASI modules first using cargo-component
RUN ./scripts/build-wasi-modules.sh

# Build the application (now that WASI bindings exist)
RUN cargo build --release -p gridway-server --bin gridway-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    netcat-traditional \
    && rm -rf /var/lib/apt/lists/*

# Create gridway user
RUN useradd -m -u 1000 -s /bin/bash gridway

# Copy binary from builder
COPY --from=builder /usr/src/gridway/target/release/gridway-server /usr/local/bin/gridway-server

# Copy WASI modules
RUN mkdir -p /usr/local/lib/gridway/wasi-modules
COPY --from=builder /usr/src/gridway/modules/*.wasm /usr/local/lib/gridway/wasi-modules/

# Create data directory
RUN mkdir -p /gridway && chown gridway:gridway /gridway

# Switch to gridway user
USER gridway

# Set working directory
WORKDIR /gridway

# Expose ports
EXPOSE 26658 9090 1317

# Set environment variables
ENV RUST_LOG=info
ENV GRIDWAY_HOME=/gridway
ENV WASI_MODULE_PATH=/usr/local/lib/gridway/wasi-modules

# Default command
CMD ["gridway-server", "start", "--home", "/gridway"]