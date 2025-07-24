//! Cryptographic primitives for gridway
//!
//! This crate provides core cryptographic types and operations using
//! well-audited implementations from the RustCrypto project.

pub mod keys;
pub mod multisig;
pub mod signature;
pub mod verify;

pub use keys::{PrivateKey, PublicKey};
pub use signature::{sign_message, verify_signature};
pub use verify::{
    create_sign_doc, verify_transaction_signature, SignDoc, SignMode, SignatureData, SignerData,
    TransactionVerifier, VerificationError,
};
