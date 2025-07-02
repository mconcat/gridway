#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Print colored output
print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_info() {
    echo -e "${YELLOW}→ $1${NC}"
}

# Detect OS
detect_os() {
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        echo "linux"
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        echo "macos"
    elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
        echo "windows"
    else
        echo "unknown"
    fi
}

# Check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Install Rust
install_rust() {
    if command_exists rustc && command_exists cargo; then
        print_success "Rust is already installed ($(rustc --version))"
    else
        print_info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        print_success "Rust installed successfully"
    fi
}

# Install system dependencies based on OS
install_system_deps() {
    local os=$(detect_os)
    
    case $os in
        "linux")
            print_info "Detected Linux system"
            if command_exists apt-get; then
                print_info "Installing system dependencies with apt..."
                sudo apt-get update
                sudo apt-get install -y \
                    build-essential \
                    pkg-config \
                    libssl-dev \
                    librocksdb-dev \
                    protobuf-compiler \
                    clang
            elif command_exists yum; then
                print_info "Installing system dependencies with yum..."
                sudo yum install -y \
                    gcc \
                    gcc-c++ \
                    pkgconfig \
                    openssl-devel \
                    rocksdb-devel \
                    protobuf-compiler \
                    clang
            elif command_exists pacman; then
                print_info "Installing system dependencies with pacman..."
                sudo pacman -S --needed \
                    base-devel \
                    pkg-config \
                    openssl \
                    rocksdb \
                    protobuf \
                    clang
            else
                print_error "Unknown Linux package manager. Please install manually:"
                echo "  - build-essential or equivalent"
                echo "  - pkg-config"
                echo "  - libssl-dev"
                echo "  - librocksdb-dev"
                echo "  - protobuf-compiler"
                echo "  - clang"
                exit 1
            fi
            print_success "System dependencies installed"
            ;;
            
        "macos")
            print_info "Detected macOS system"
            if ! command_exists brew; then
                print_info "Installing Homebrew..."
                /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
            fi
            
            print_info "Installing system dependencies with Homebrew..."
            brew install rocksdb protobuf
            print_success "System dependencies installed"
            ;;
            
        "windows")
            print_error "Windows detected. Please use WSL2 or install dependencies manually:"
            echo "  - Install Visual Studio Build Tools"
            echo "  - Install RocksDB"
            echo "  - Install Protocol Buffers"
            exit 1
            ;;
            
        *)
            print_error "Unknown operating system. Please install dependencies manually."
            exit 1
            ;;
    esac
}

# Install Rust components
install_rust_components() {
    print_info "Installing Rust components..."
    rustup component add rustfmt clippy
    print_success "Rust components installed"
}

# Install cargo dependencies
install_cargo_deps() {
    print_info "Fetching Cargo dependencies..."
    cargo fetch
    print_success "Cargo dependencies fetched"
}

# Install optional development tools
install_dev_tools() {
    read -p "Install optional development tools? (cargo-watch, cargo-audit, cargo-outdated) [y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        print_info "Installing development tools..."
        cargo install cargo-watch cargo-audit cargo-outdated
        print_success "Development tools installed"
    fi
}

# Build the project
build_project() {
    read -p "Build the project now? [Y/n] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Nn]$ ]]; then
        print_info "Building Helium..."
        cargo build --release
        print_success "Build complete"
    fi
}

# Main setup flow
main() {
    echo "======================================"
    echo "  Helium Development Setup"
    echo "======================================"
    echo
    
    # Check if we're in the right directory
    if [ ! -f "Cargo.toml" ]; then
        print_error "This script must be run from the Helium project root directory"
        exit 1
    fi
    
    # Run setup steps
    install_rust
    install_system_deps
    install_rust_components
    install_cargo_deps
    install_dev_tools
    build_project
    
    echo
    echo "======================================"
    print_success "Setup complete!"
    echo
    echo "Next steps:"
    echo "  - Run 'make build' to build the project"
    echo "  - Run 'make test' to run tests"
    echo "  - Run 'make run' to start the application"
    echo "  - Run 'make help' to see all available commands"
    echo "======================================"
}

# Run main function
main