# Helium Keyring Architecture

## Overview

The helium-keyring crate provides secure key management functionality for the Helium blockchain, supporting multiple storage backends and HD wallet derivation. This crate must balance security, usability, and compatibility with both traditional environments and WASI components.

## Design Philosophy

### Multiple Backend Support

The keyring supports multiple storage backends to accommodate different deployment scenarios:

1. **File Backend**: Encrypted file storage for development and testing
2. **OS Keyring Backend**: Integration with platform keyrings (macOS Keychain, Linux Secret Service, Windows Credential Manager)
3. **Memory Backend**: Transient storage for testing
4. **WASI-Compatible Backend**: Future support for key storage within WASI components

### Security Model

Keys are never stored in plaintext. The security model includes:

- Encryption at rest for all backends
- Password-based key derivation (PBKDF2/Argon2)
- Secure memory handling to prevent key material leakage
- Integration with hardware security modules (future)

## Key Features

### HD Wallet Support

Full BIP32/BIP39/BIP44 support for hierarchical deterministic wallets:

```rust
// Derive keys using standard paths
let key = keyring.derive("m/44'/118'/0'/0/0")?;
```

### Multi-Algorithm Support

Support for multiple signing algorithms:
- Ed25519 (validators only, following Cosmos SDK)
- Secp256k1 (user accounts)
- Future: BLS12-381 for aggregated signatures

### Key Import/Export

Secure import and export functionality:
- Mnemonic phrase import/export
- Encrypted key file import/export
- Armor format for secure transmission

## WASI Considerations

### Capability-Based Access

In WASI environments, key access is mediated through capabilities:

```rust
// Keys accessed through VFS paths
/keys/validator/consensus
/keys/accounts/alice
```

### Component Integration

When running as a WASI component, the keyring:
- Cannot access OS keyrings directly
- Must use VFS-based storage
- Relies on host-provided cryptographic primitives

## Integration Points

### With helium-crypto

The keyring crate builds on helium-crypto primitives:
- Uses PublicKey/PrivateKey types
- Delegates signing operations
- Shares algorithm support

### With WASI Components

Components access keys through host functions:
```rust
// Host function for signing
fn host_sign(key_name: &str, message: &[u8]) -> Result<Signature>;
```

## Future Enhancements

1. **Hardware Security Module (HSM) Support**: Integration with YubiHSM, Ledger, etc.
2. **Threshold Signatures**: Support for multi-party computation
3. **Remote Signing**: Network-based signing services
4. **Key Rotation**: Automated key rotation policies

## Security Considerations

1. **Memory Safety**: Use zeroize for secure memory cleanup
2. **Side-Channel Resistance**: Constant-time operations where applicable
3. **Audit Trail**: Log all key operations for security monitoring
4. **Access Control**: Fine-grained permissions for key operations

## Testing Strategy

1. **Backend Abstraction Tests**: Ensure all backends behave identically
2. **Security Tests**: Attempt common attacks (key extraction, timing attacks)
3. **Integration Tests**: Test with actual signing operations
4. **WASI Tests**: Verify behavior in sandboxed environments