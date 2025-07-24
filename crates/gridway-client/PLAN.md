# Gridway Client Architecture

This document details the architectural vision of the Gridway Client crate, which provides client-side infrastructure for interacting with Gridway blockchain nodes. Following the Cosmos SDK client package design, this crate focuses on transaction building, broadcasting, key management, and CLI tools—making the underlying WASI component architecture transparent to end users.

## Design Philosophy

The client crate serves as the bridge between users and the blockchain, providing familiar interfaces that hide the complexity of the component-based architecture. It maintains strict compatibility with Cosmos SDK standards while offering a superior developer experience through Rust's type safety and modern tooling.

## Core Components

### Client Context

The heart of the client architecture is the context management system, which maintains connection information and configuration for all client operations:

```rust
pub struct ClientContext {
    node_uri: String,
    chain_id: String,
    keyring: Arc<Keyring>,
    broadcast_mode: BroadcastMode,
    gas_adjustment: f64,
    gas_prices: Option<String>,
}

impl ClientContext {
    // Manages the lifecycle of client connections
    pub async fn with_node(&self, f: impl FnOnce(&NodeClient)) -> Result<(), Error> {
        let client = NodeClient::connect(&self.node_uri).await?;
        f(&client);
        Ok(())
    }
}
```

This design ensures that all client operations have consistent configuration and proper resource management.

### Transaction Building

The transaction builder provides a fluent interface for constructing blockchain transactions:

```rust
pub struct TxBuilder {
    client: NodeClient,
    chain_id: String,
    messages: Vec<Any>,
    fee: Option<Fee>,
    memo: String,
    timeout_height: u64,
}

impl TxBuilder {
    pub fn new(client: NodeClient) -> Self { /* ... */ }
    
    pub fn with_messages(mut self, msgs: Vec<Any>) -> Self {
        self.messages = msgs;
        self
    }
    
    pub fn with_fee(mut self, fee: Fee) -> Self {
        self.fee = Some(fee);
        self
    }
    
    pub async fn build_and_sign(&self, signer: &Address) -> Result<Transaction, Error> {
        // Auto-fetch account info
        let account_info = self.client.get_account(signer).await?;
        
        // Build sign doc with proper sequence and account number
        let sign_doc = self.build_sign_doc(account_info)?;
        
        // Sign with keyring
        let signature = self.client.keyring.sign(signer, &sign_doc)?;
        
        Ok(self.assemble_tx(sign_doc, signature))
    }
}
```

Key features:
- **Builder Pattern**: Intuitive transaction construction
- **Auto Account Fetching**: No manual sequence/account number management
- **Gas Estimation**: Automatic or manual gas configuration
- **Type Safety**: Compile-time validation of transaction structure

### Query Infrastructure

The client provides comprehensive query capabilities through both REST and gRPC:

```rust
pub struct QueryClient {
    http_client: reqwest::Client,
    base_url: String,
}

impl QueryClient {
    // ABCI queries for direct state access
    pub async fn abci_query(&self, path: &str, data: Vec<u8>) -> Result<AbciResponse, Error> {
        let response = self.http_client
            .get(&format!("{}/abci_query", self.base_url))
            .query(&[("path", path), ("data", hex::encode(data))])
            .send()
            .await?;
        
        response.json().await
    }
    
    // High-level query methods
    pub async fn get_balance(&self, address: &Address, denom: &str) -> Result<Coin, Error> {
        self.query_module("bank", "Balance", BalanceRequest { address, denom }).await
    }
    
    pub async fn get_account(&self, address: &Address) -> Result<BaseAccount, Error> {
        self.query_module("auth", "Account", AccountRequest { address }).await
    }
}
```

The query infrastructure automatically handles:
- Path construction for different module queries
- Response parsing and type conversion
- Error handling with descriptive messages
- Height-specific queries for historical data

## Command-Line Interface

The CLI provides a comprehensive interface for all blockchain operations:

### CLI Architecture

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gridway")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    
    #[arg(long, global = true)]
    pub node: Option<String>,
    
    #[arg(long, global = true)]
    pub chain_id: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize node configuration
    Init {
        #[arg(long)]
        home: Option<PathBuf>,
    },
    
    /// Key management commands
    Keys(KeyCommands),
    
    /// Transaction commands
    Tx(TxCommands),
    
    /// Query commands  
    Query(QueryCommands),
    
    /// Start the node
    Start {
        #[arg(long)]
        home: Option<PathBuf>,
    },
}
```

Key features:
- **Hierarchical Commands**: Organized by functional area (keys, tx, query)
- **Global Options**: Node URL and chain ID available to all subcommands
- **Configuration Integration**: Reads from config files with CLI overrides
- **Interactive Mode**: Prompts for passwords and confirmations when needed

### Key Management

The CLI integrates with gridway-keyring for secure key operations:

```bash
# Add a new key
gridway keys add alice

# Import existing key
gridway keys import bob --recover

# List all keys
gridway keys list

# Export for backup
gridway keys export alice --unarmored-hex --unsafe
```

### Transaction Operations

Transaction commands follow Cosmos SDK patterns:

```bash
# Send tokens
gridway tx bank send alice cosmos1... 1000uatom --fees 1000uatom

# Sign offline
gridway tx sign tx.json --from alice --offline

# Broadcast pre-signed
gridway tx broadcast signed-tx.json

# Multisig operations
gridway tx multisign tx.json alice bob --offline
```

### Query Operations

Comprehensive query support for all modules:

```bash
# Account queries
gridway query account cosmos1...
gridway query balance cosmos1... uatom

# Transaction queries
gridway query tx <hash>
gridway query txs --events 'transfer.recipient=cosmos1...'

# Block queries
gridway query block <height>
gridway query block-results <height>
```

## Integration with Component Architecture

While the client crate abstracts away the WASI component complexity, it leverages the dynamic service generation provided by the server:

### Dynamic Module Discovery

The client can discover available modules and their operations through the server's API:

```rust
impl Client {
    pub async fn list_modules(&self) -> Result<Vec<ModuleInfo>, Error> {
        // Query server for dynamically generated module endpoints
        let modules = self.query("/gridway/modules/v1/list").await?;
        Ok(modules)
    }
    
    pub async fn get_module_methods(&self, module: &str) -> Result<ModuleMethods, Error> {
        // Discover available queries and transactions for a module
        let methods = self.query(&format!("/gridway/modules/v1/{}/methods", module)).await?;
        Ok(methods)
    }
}
```

This enables:
- **Dynamic CLI Generation**: CLI commands can be built from discovered modules
- **SDK Flexibility**: Client libraries adapt to available modules
- **Backwards Compatibility**: New modules appear automatically in clients

### Configuration Management

The client manages configuration through a layered approach:

```toml
# ~/.gridway/config.toml
[client]
node = "http://localhost:26657"
chain_id = "gridway-1"
broadcast_mode = "sync"

[keyring]
backend = "file"
dir = "~/.gridway/keys"

[gas]
adjustment = 1.5
prices = "0.025uatom"
```

Configuration sources in priority order:
1. Command-line flags
2. Environment variables
3. Configuration file
4. Built-in defaults

## Future Enhancements

### Hardware Wallet Integration

Future support for hardware wallets through standard interfaces:

```rust
trait WalletProvider {
    async fn get_address(&self, path: &DerivationPath) -> Result<Address, Error>;
    async fn sign(&self, path: &DerivationPath, msg: &[u8]) -> Result<Signature, Error>;
}

// Implementations for different hardware wallets
struct LedgerWallet { /* ... */ }
struct TrezorWallet { /* ... */ }
```

### Multi-Language SDKs

While the Rust client provides the foundation, additional SDKs will support broader ecosystem adoption:

- **TypeScript SDK**: For web applications and Node.js
- **Python SDK**: For data analysis and scripting
- **Go SDK**: For projects migrating from Cosmos SDK

Each SDK will maintain the same architectural principles while providing idiomatic interfaces for their respective languages.

## Architectural Principles

The client crate maintains several key principles:

1. **Abstraction**: Hide WASI component complexity from end users
2. **Compatibility**: Maintain Cosmos SDK standards for wallets and tools
3. **Type Safety**: Leverage Rust's type system for correctness
4. **Modularity**: Clear separation between client, CLI, and key management
5. **Discoverability**: Dynamic adaptation to available blockchain modules

## See Also

- [Server Architecture](../gridway-server/PLAN.md) - Dynamic service generation
- [Keyring Management](../gridway-keyring/PLAN.md) - Key storage and management
- [Project Overview](../../PLAN.md) - High-level architectural vision