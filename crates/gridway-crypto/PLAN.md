# Helium Crypto Architecture

This document details the architectural vision of the Helium Crypto crate, which provides cryptographic utility functions maintaining strict compatibility with Cosmos SDK standards. This crate serves as a foundational library for cryptographic operations, remaining independent of the WASI component architecture while providing essential primitives for the blockchain.

## Design Philosophy

The crypto crate embraces simplicity and correctness over complex abstractions. It provides pure cryptographic functions without state management, key storage, or component interfaces. This separation of concerns ensures that cryptographic operations remain auditable, testable, and reusable across all layers of the system.

## Supported Key Types

Following Cosmos SDK compatibility requirements, Helium supports two primary key types with distinct purposes:

### Secp256k1 - User Accounts
The primary key type for user accounts and transaction signing, maintaining compatibility with the broader blockchain ecosystem:

```rust
pub enum PrivateKey {
    Secp256k1(Secp256k1PrivKey),
}

pub enum PublicKey {
    Secp256k1(Secp256k1PubKey),
}

impl PublicKey {
    pub fn to_address(&self) -> Address {
        match self {
            PublicKey::Secp256k1(key) => {
                // Cosmos SDK address derivation: RIPEMD160(SHA256(compressed_pubkey))
                let compressed = key.to_encoded_point(true).as_bytes().to_vec();
                let sha256_hash = sha2::Sha256::digest(&compressed);
                let ripemd160_hash = ripemd::Ripemd160::digest(&sha256_hash);
                Address::from_bytes(&ripemd160_hash)
            }
        }
    }
}
```

Key properties:
- **Address Derivation**: Uses Cosmos SDK standard `RIPEMD160(SHA256(compressed_pubkey))`
- **Bech32 Encoding**: Supports standard `cosmos1...` address format
- **Ecosystem Compatibility**: Works with existing wallets, explorers, and tools

### Ed25519 - Validator Consensus
Exclusively for validator operations and consensus participation, following Cosmos SDK's architectural separation:

```rust
pub enum ValidatorKey {
    Ed25519(Ed25519PrivKey),
}

// Ed25519 keys are used only for consensus validation
// They must NOT be used for user accounts or transaction signing
impl ValidatorKey {
    pub fn to_consensus_address(&self) -> ConsensusAddress {
        // First 20 bytes of SHA256 hash, per Tendermint/CometBFT spec
        let public_bytes = self.public_key().to_bytes();
        let hash = sha2::Sha256::digest(&public_bytes);
        ConsensusAddress::from_bytes(&hash[..20])
    }
}

## Multi-Signature Support

Multi-signature functionality provides threshold signature capabilities for enhanced security and governance operations:

```rust
pub struct MultisigPublicKey {
    threshold: u32,
    public_keys: Vec<PublicKey>,
}

impl MultisigPublicKey {
    pub fn to_address(&self) -> Address {
        // Cosmos SDK compatible multisig address derivation
        // Address derived from sorted public keys and threshold
        let mut sorted_keys = self.public_keys.clone();
        sorted_keys.sort_by_key(|k| k.to_bytes());
        
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.threshold.to_be_bytes());
        for key in &sorted_keys {
            hasher.update(key.to_bytes());
        }
        
        let sha256_hash = hasher.finalize();
        let ripemd160_hash = ripemd::Ripemd160::digest(&sha256_hash);
        Address::from_bytes(&ripemd160_hash)
    }
}
```

Key features:
- **Threshold Signatures**: Supports m-of-n signature schemes
- **Deterministic Addresses**: Consistent address generation from sorted keys
- **Cosmos Compatibility**: Matches SDK's multisig address derivation

## Host Crypto Functions for WASI Components

A critical architectural consideration is how WASI components access cryptographic operations. Since cryptographic functions are computationally intensive and security-critical, they remain in the host environment rather than being reimplemented in each component.

### Access Patterns Under Consideration

The architecture supports multiple potential patterns for component crypto access:

```rust
// Option 1: VFS-based crypto proxy
// Components could access crypto through special files
/dev/crypto/sign   -> Write message, read signature
/dev/crypto/verify -> Write message+signature, read result

// Option 2: Dedicated WASI crypto world
interface crypto {
    sign: func(key-id: string, message: list<u8>) -> result<signature, error>;
    verify: func(pubkey: public-key, message: list<u8>, sig: signature) -> bool;
    hash: func(algorithm: hash-algo, data: list<u8>) -> list<u8>;
}

// Option 3: Host function imports
// Direct host functions with minimal gas cost
```

The key design goals for component crypto access:
- **Minimal Gas Overhead**: Crypto operations should not be prohibitively expensive
- **Security Isolation**: Components cannot access private keys directly
- **Standardization**: All components use the same crypto interfaces
- **Auditability**: All crypto operations can be logged and monitored

The final implementation pattern will be determined based on performance benchmarks and security analysis, ensuring that components can efficiently perform necessary cryptographic operations without compromising the security model.

## Future Enhancements

### BLS Signature Aggregation

BLS (Boneh-Lynn-Shacham) signatures offer unique properties that could significantly improve validator efficiency:

```rust
// Future BLS support for validator aggregation
pub enum AggregateSignature {
    BLS(BlsAggregateSignature),
}

// BLS enables combining multiple signatures into one
// Reducing block header size and verification time
impl BlsAggregateSignature {
    pub fn aggregate(signatures: Vec<BlsSignature>) -> Self {
        // Combine multiple validator signatures into a single proof
    }
}
```

BLS signatures would enable:
- **Compact Block Headers**: Aggregate all validator signatures into one
- **Efficient Light Clients**: Verify consensus with a single signature check  
- **Reduced Network Overhead**: Smaller block propagation size

## Architectural Principles

The crypto crate maintains several key principles:

1. **Pure Functions**: No global state or side effects
2. **Cosmos Compatibility**: Strict adherence to SDK standards
3. **Algorithm Separation**: Clear distinction between user keys (secp256k1) and validator keys (ed25519)
4. **Extensibility**: Enum-based design allows adding new algorithms
5. **Security First**: Correctness over performance optimization

## See Also

- [Keyring Architecture](../helium-keyring/PLAN.md) - Key management, storage, and HSM integration
- [BaseApp Component Model](../helium-baseapp/PLAN.md) - How components access crypto functions
- [Project Overview](../../PLAN.md) - High-level architectural vision